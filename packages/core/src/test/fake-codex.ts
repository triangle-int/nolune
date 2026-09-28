import { chmodSync, existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { updateConfig } from '../config.ts';
import { paths } from '../paths.ts';

/*
 * A stand-in for Codex's app server, for its ChatGPT sign-in, which can't be done without OpenAI.
 * For the tests of nolune's side of the sign-in, in core and in the CLI.
 */

/**
 * Speaks enough of Codex's app server for the sign-in: its account lives in `account.json` in
 * the Codex home nolune gives it, the code counts as entered once `entered` is there, and every
 * request is written to `requests.jsonl`.
 */
const FAKE_CODEX = `#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const home = process.env.CODEX_HOME;
const file = (name) => path.join(home, name);
const account = () => (fs.existsSync(file('account.json')) ? JSON.parse(fs.readFileSync(file('account.json'), 'utf8')) : null);
const send = (m) => process.stdout.write(JSON.stringify(m) + '\\n');
let login = null;
setInterval(() => {
	if (login && fs.existsSync(file('entered'))) {
		fs.writeFileSync(file('account.json'), JSON.stringify({ type: 'chatgpt', email: 'anna@example.com', planType: 'plus' }));
		send({ method: 'account/login/completed', params: { loginId: login, success: true, error: null } });
		login = null;
	}
}, 20);
let buffer = '';
process.stdin.on('data', (chunk) => {
	buffer += chunk;
	let at;
	while ((at = buffer.indexOf('\\n')) >= 0) {
		const m = JSON.parse(buffer.slice(0, at));
		buffer = buffer.slice(at + 1);
		fs.appendFileSync(file('requests.jsonl'), JSON.stringify({ method: m.method, params: m.params ?? null, args: process.argv.slice(2) }) + '\\n');
		if (m.id === undefined) continue;
		const result = (() => {
			switch (m.method) {
				case 'initialize': return { userAgent: 'fake', codexHome: home };
				case 'account/read': return { account: account(), requiresOpenaiAuth: true };
				case 'account/login/start':
					login = 'login-1';
					return { type: 'chatgptDeviceCode', loginId: login, verificationUrl: 'https://auth.openai.com/codex/device', userCode: 'ABCD-1234' };
				case 'account/login/cancel':
					send({ method: 'account/login/completed', params: { loginId: login, success: false, error: 'cancelled' } });
					login = null;
					return {};
				case 'account/logout':
					fs.rmSync(file('account.json'), { force: true });
					return {};
				case 'model/list':
					return { data: [
						{ id: 'gpt-6-astra', displayName: 'GPT-6 Astra', hidden: false, isDefault: true },
						{ id: 'gpt-5.4', displayName: 'GPT-5.4', hidden: true, isDefault: false }
					], nextCursor: null };
			}
			return null;
		})();
		send(result ? { id: m.id, result } : { id: m.id, error: { code: -32601, message: 'no ' + m.method } });
	}
});
process.stdin.on('end', () => process.exit(0));
`;

/** Has nolune run the stand-in, signed in with `account` when there's one. */
export function useFakeCodex(account: object | null = null): void {
	const dir = mkdtempSync(join(tmpdir(), 'nolune-fake-codex-'));
	const path = join(dir, 'codex');
	writeFileSync(path, FAKE_CODEX);
	chmodSync(path, 0o755);
	updateConfig((c) => {
		c.codexPath = path;
	});
	if (account) writeFileSync(join(paths.codexHome, 'account.json'), JSON.stringify(account));
}

/** What the stand-in was sent, in order. */
export function codexRequests(): { method: string; params: Record<string, unknown> | null }[] {
	const log = join(paths.codexHome, 'requests.jsonl');
	if (!existsSync(log)) return [];
	return readFileSync(log, 'utf8')
		.trim()
		.split('\n')
		.map((line) => JSON.parse(line));
}
