import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { CODEX_LOGIN_POLL_MS, codexAccountLabel, codexErrorCopy, codexReady, codexView, loginInstructions, loginProgress, offersDeviceCode } from '../src/lib/models/codex.js';

// What `GET /api/config/codex/status` answers (#27), one status per state
// the tile shows. Nothing here carries a token; the guard in
// server/tests/codex_auth.rs keeps the client's types that way too.
const PIN = '0.155.0';
const ready = { state: 'ready', pinned_version: PIN, path: '/opt/homebrew/bin/codex', version: PIN };
const base = { binary: ready, installed: true, compatible: true, logged_in: false, account: null, login: null };
const chatgpt = { kind: 'chatgpt', email: 'companion@example.test', plan: 'plus' };
const device = { id: 'login_1', method: 'device_code', state: 'pending', verification_url: 'https://auth.openai.com/codex/device', user_code: 'NLNE-FXTR' };
const browser = { id: 'login_2', method: 'browser', state: 'pending', auth_url: 'https://auth.openai.com/oauth/authorize?state=fixture' };

const statuses = {
	loading: null,
	notInstalled: { binary: { state: 'not_installed', pinned_version: PIN, message: 'codex is not installed: no `codex` on PATH and NOLUNE_CODEX_BIN is unset' }, installed: false, compatible: false, logged_in: false, account: null, login: null },
	incompatible: { binary: { state: 'incompatible', pinned_version: PIN, path: '/usr/local/bin/codex', version: '0.154.0', message: '/usr/local/bin/codex is codex 0.154.0; Nolune supports codex 0.155.0 only' }, installed: true, compatible: false, logged_in: false, account: null, login: null },
	unusable: { binary: { state: 'unusable', pinned_version: PIN, path: '/usr/local/bin/codex', message: '/usr/local/bin/codex cannot report its version: --version failed (1): boom' }, installed: true, compatible: false, logged_in: false, account: null, login: null },
	unavailable: { ...base, error: 'codex app-server handshake failed: exited with status 1' },
	loggedOut: base,
	pendingDevice: { ...base, login: device },
	pendingBrowser: { ...base, login: browser },
	failed: { ...base, login: { id: 'login_1', method: 'device_code', state: 'failed', error: 'device code expired before it was entered' } },
	loggedIn: { ...base, logged_in: true, account: chatgpt, login: { id: 'login_1', method: 'device_code', state: 'completed' } },
	keyedAccount: { ...base, logged_in: true, account: { kind: 'api_key' } },
};

test('the tile reads one state per status, with a headline, what to do, and which actions apply (#27)', () => {
	const view = (name) => codexView(statuses[name]);

	assert.equal(view('loading').state, 'loading');
	assert.equal(view('loading').canLogin, false);
	assert.equal(view('loading').canLogout, false);

	const missing = view('notInstalled');
	assert.equal(missing.state, 'not_installed');
	assert.match(missing.headline, /not installed/i);
	assert.match(missing.detail, /NOLUNE_CODEX_BIN/, 'the server message says how to point at a binary');
	assert.match(missing.detail, new RegExp(PIN.replace(/\./g, '\\.')), 'the pinned release is named');
	assert.match(missing.detail, /is unset\. Install codex/, 'the server sentence is ended before the hint');
	assert.equal(missing.canLogin, false);
	assert.equal(missing.canLogout, false);
	assert.equal(missing.instructions, null);

	const older = view('incompatible');
	assert.equal(older.state, 'incompatible');
	assert.match(older.headline, /0\.154\.0/);
	assert.match(older.headline, /0\.155\.0/);
	assert.match(older.detail, /\/usr\/local\/bin\/codex/);
	assert.equal(older.canLogin, false);

	const broken = view('unusable');
	assert.equal(broken.state, 'unusable');
	assert.match(broken.headline, /cannot run|could not/i);
	assert.match(broken.detail, /--version failed/);
	assert.equal(broken.canLogin, false);

	const gone = view('unavailable');
	assert.equal(gone.state, 'unavailable');
	assert.match(gone.headline, /could not answer|did not answer/i);
	assert.match(gone.detail, /handshake failed: exited with status 1\. Logging in/);
	assert.equal(gone.canLogin, true, 'a retry starts the child again');
	assert.equal(gone.canLogout, false);

	const out = view('loggedOut');
	assert.equal(out.state, 'logged_out');
	assert.match(out.headline, /not logged in/i);
	assert.equal(out.canLogin, true);
	assert.equal(out.canLogout, false);
	assert.match(out.binary, /codex 0\.155\.0/);
	assert.match(out.binary, /\/opt\/homebrew\/bin\/codex/);

	const pending = view('pendingDevice');
	assert.equal(pending.state, 'pending');
	assert.equal(pending.canLogin, false, 'one login at a time');
	assert.equal(pending.canLogout, true, 'logging out cancels the pending login');
	assert.deepEqual(pending.instructions, loginInstructions(device));

	const failed = view('failed');
	assert.equal(failed.state, 'failed');
	assert.match(failed.headline, /login failed/i);
	assert.match(failed.detail, /device code expired/);
	assert.equal(failed.canLogin, true);
	assert.equal(failed.instructions, null, 'the code is gone once the login is over');

	const on = view('loggedIn');
	assert.equal(on.state, 'logged_in');
	assert.match(on.headline, /logged in as companion@example\.test/i);
	assert.match(on.headline, /Plus/);
	assert.equal(on.canLogin, false);
	assert.equal(on.canLogout, true);
	assert.equal(on.instructions, null);

	const keyed = view('keyedAccount');
	assert.equal(keyed.state, 'logged_in');
	assert.match(keyed.headline, /API key/);
	assert.match(keyed.headline, /codex/i);
});

test('the account label is the email and plan, or the kind when there is no email', () => {
	assert.equal(codexAccountLabel(chatgpt), 'companion@example.test · Plus');
	assert.equal(codexAccountLabel({ kind: 'chatgpt', email: 'a@b.c' }), 'a@b.c');
	assert.equal(codexAccountLabel({ kind: 'chatgpt' }), 'ChatGPT');
	assert.equal(codexAccountLabel({ kind: 'api_key' }), 'an API key codex holds');
	assert.equal(codexAccountLabel({ kind: 'other' }), 'other');
	assert.equal(codexAccountLabel(null), '');
});

test('login instructions are the URL and the code for a device code, the URL alone for the browser flow', () => {
	const code = loginInstructions(device);
	assert.equal(code.url, 'https://auth.openai.com/codex/device');
	assert.equal(code.code, 'NLNE-FXTR');
	assert.match(code.note, /any device|other device|phone/i);
	const managed = loginInstructions(browser);
	assert.equal(managed.url, browser.auth_url);
	assert.equal(managed.code, null);
	assert.match(managed.note, /same machine|machine this server runs on|server's machine/i);
	// Only a pending login has instructions.
	assert.equal(loginInstructions({ ...device, state: 'completed' }), null);
	assert.equal(loginInstructions({ ...device, state: 'failed', error: 'x' }), null);
	assert.equal(loginInstructions(null), null);
});

test('polling reads whether the login it started is still pending, done, failed or replaced', () => {
	assert.equal(loginProgress(statuses.pendingDevice, 'login_1'), 'pending');
	assert.equal(loginProgress(statuses.loggedIn, 'login_1'), 'completed');
	assert.equal(loginProgress(statuses.failed, 'login_1'), 'failed');
	// Another login took its place, or a logout cleared it.
	assert.equal(loginProgress(statuses.pendingBrowser, 'login_1'), 'replaced');
	assert.equal(loginProgress(statuses.loggedOut, 'login_1'), 'replaced');
	// The app-server went away mid-login: not pending any more.
	assert.equal(loginProgress({ ...statuses.pendingDevice, error: 'codex app-server exited' }, 'login_1'), 'failed');
	assert.ok(CODEX_LOGIN_POLL_MS >= 1000 && CODEX_LOGIN_POLL_MS <= 5000, 'polls every few seconds');
});

test('a login codex holds is the outcome whatever the pending record says (`codex login` run on the server)', () => {
	// The server finishes a login record only on the app-server's event for
	// that id; a login done outside (`codex login` in a terminal) leaves the
	// record pending while the status already reports the account.
	const outside = { ...statuses.pendingDevice, logged_in: true, account: chatgpt };
	assert.equal(loginProgress(outside, 'login_1'), 'completed');
	assert.equal(loginProgress({ ...statuses.pendingBrowser, logged_in: true, account: chatgpt }, 'login_2'), 'completed');
	// The record is gone (a restart cleared it) but the login is there.
	assert.equal(loginProgress({ ...base, logged_in: true, account: chatgpt, login: null }, 'login_1'), 'completed');
	// Another login took the id, and it is the one that finished: still a login to use.
	assert.equal(loginProgress({ ...statuses.loggedIn, login: { id: 'login_9', method: 'browser', state: 'completed' } }, 'login_1'), 'completed');
	// Without the account nothing changes: pending stays pending, gone stays replaced.
	assert.equal(loginProgress(statuses.pendingDevice, 'login_1'), 'pending');
	assert.equal(loginProgress(statuses.loggedOut, 'login_1'), 'replaced');
	assert.equal(codexReady(outside), true, 'the same status passes the onboarding gate');
});

test('a pending browser-flow login can be swapped for a device code; a device code already is one', () => {
	// The managed flow needs a browser on the machine the server runs on;
	// a person onboarding from another device has none, so the login stage
	// offers the device code instead of waiting out the deadline.
	assert.equal(offersDeviceCode(browser), true);
	assert.equal(offersDeviceCode(device), false);
	assert.equal(offersDeviceCode({ ...browser, state: 'completed' }), false);
	assert.equal(offersDeviceCode({ ...browser, state: 'failed', error: 'x' }), false);
	assert.equal(offersDeviceCode(null), false);
	assert.equal(offersDeviceCode(undefined), false);
});

test('ready means the pinned binary, a login, and an app-server that answered', () => {
	assert.equal(codexReady(statuses.loggedIn), true);
	assert.equal(codexReady(statuses.keyedAccount), true);
	assert.equal(codexReady(statuses.loggedOut), false);
	assert.equal(codexReady(statuses.pendingDevice), false);
	assert.equal(codexReady(statuses.notInstalled), false);
	assert.equal(codexReady({ ...statuses.loggedIn, error: 'codex app-server exited' }), false);
	assert.equal(codexReady(null), false);
});

test('a typed login or logout error reads as one sentence with what to do', () => {
	const cases = [
		['codex_not_installed', 'codex is not installed: no `codex` on PATH and NOLUNE_CODEX_BIN is unset', /not installed.*NOLUNE_CODEX_BIN/],
		['codex_incompatible', '/usr/local/bin/codex is codex 0.154.0; Nolune supports codex 0.155.0 only', /0\.154\.0.*0\.155\.0/],
		['codex_unusable', '/usr/local/bin/codex cannot report its version: boom', /cannot report its version/],
		['codex_unavailable', 'codex app-server handshake failed: exited', /handshake failed.*try again/i],
		['codex_refused', 'codex app-server error -32000: login already in flight', /already in flight/],
	];
	for (const [error, message, pattern] of cases) {
		const text = codexErrorCopy({ ok: false, error, message, status: 503 });
		assert.match(text, pattern, `${error}: ${text}`);
		assert.doesNotMatch(text, /API key|token/i, error);
	}
	// An error the client does not know shows the server's message as it is.
	assert.equal(codexErrorCopy({ ok: false, error: 'something_new', message: 'the server said this', status: 500 }), 'the server said this');
	assert.equal(codexErrorCopy({ ok: false, error: 'something_new', message: '', status: 500 }), 'Codex could not answer.');
});

test('nothing in the codex module names a token-bearing field', () => {
	const source = readFileSync(new URL('../src/lib/models/codex.js', import.meta.url), 'utf8');
	for (const banned of ['token', 'secret', 'password', 'cookie', 'api_key:', 'apiKey', 'auth.json', 'Bearer']) {
		assert.doesNotMatch(source, new RegExp(banned.replace(/\./g, '\\.'), 'i'), banned);
	}
});
