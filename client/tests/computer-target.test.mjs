import test from 'node:test';
import assert from 'node:assert/strict';
import { buildSpaces, homeSpace } from '../src/lib/computers/spaces.js';
import {
	NO_TARGET,
	normalizeTarget,
	requestTarget,
	runningLabel,
	targetOptions,
	targetStorageKey,
	targetSummary,
} from '../src/lib/computers/target.js';

const NOW = 1_767_603_600;

function desktop(machine_id, hostname, overrides = {}) {
	return {
		machine_id,
		display_name: hostname,
		custom_name: null,
		hostname,
		os: 'macos',
		platform: 'macos',
		location: 'desktop',
		screen_width: 1440,
		screen_height: 900,
		permissions: { accessibility: 'granted', screen_capture: 'granted' },
		capabilities: ['screenshot'],
		first_seen: NOW - 3600,
		last_seen: NOW,
		instance_slug: 'companion',
		online: true,
		health: 'healthy',
		driver_version: null,
		cua_health: null,
		...overrides,
	};
}

const home = homeSpace({ connected: true, address: 'localhost:26559', version: '0.20.0', companionName: 'Luna', nowSeconds: NOW });
const studio = desktop('studio-id', 'studio', { display_name: 'Studio Mac', custom_name: 'Studio Mac' });
const laptop = desktop('laptop-id', 'laptop');
const den = desktop('den-id', 'den', { last_seen: NOW - 300, online: false, health: 'unavailable' });
const kiosk = desktop('kiosk-id', 'kiosk', { permissions: { accessibility: 'denied', screen_capture: 'granted' } });

const spaces = (...machines) => buildSpaces(machines, NOW, home, 'Luna');

test('options list the server home first, then every desktop with its state', () => {
	const options = targetOptions(spaces(studio, den, laptop, kiosk));
	assert.deepEqual(
		options.map((o) => [o.value, o.label, o.detail, o.kind]),
		[
			['server-home', 'Luna’s home', 'This server', 'home'],
			['kiosk-id', 'kiosk', 'Needs permission', 'desktop'],
			['laptop-id', 'laptop', 'Online', 'desktop'],
			['studio-id', 'Studio Mac', 'Online', 'desktop'],
			['den-id', 'den', 'Offline', 'desktop'],
		],
	);
	assert.deepEqual(
		options.map((o) => o.status),
		['online', 'restricted', 'online', 'online', 'offline'],
	);
});

test('a stale choice is dropped, a known one is kept', () => {
	const rows = spaces(studio, laptop);
	assert.equal(normalizeTarget('studio-id', rows), 'studio-id');
	assert.equal(normalizeTarget('server-home', rows), 'server-home');
	assert.equal(normalizeTarget('forgotten-id', rows), NO_TARGET);
	assert.equal(normalizeTarget(null, rows), NO_TARGET);
	assert.equal(normalizeTarget(undefined, rows), NO_TARGET);
});

test('a remembered computer is kept as it is until the first listing arrives', () => {
	// No listing yet (`null`): not listed is not forgotten. The request carries
	// the remembered id, so a computer that turns out to be offline is refused
	// by the server by name instead of being replaced by the only connected one.
	assert.equal(normalizeTarget('laptop-id', null), 'laptop-id');
	assert.equal(normalizeTarget('server-home', null), 'server-home');
	assert.equal(normalizeTarget(NO_TARGET, null), NO_TARGET);
	assert.equal(normalizeTarget(null, null), NO_TARGET);
	assert.equal(requestTarget(normalizeTarget('laptop-id', null)), 'laptop-id');
	assert.deepEqual(targetOptions(null), []);
	// The trigger says the choice is kept while the listing is on its way.
	assert.deepEqual(targetSummary('laptop-id', null), {
		name: 'Remembered computer',
		detail: 'Checking which computers are connected',
		status: 'pending',
		ambiguous: false,
	});
	assert.deepEqual(targetSummary(NO_TARGET, null), {
		name: 'Ask me',
		detail: 'Checking which computers are connected',
		status: 'pending',
		ambiguous: false,
	});
	// Nothing is certain enough for the chat bar to name.
	assert.equal(runningLabel('laptop-id', null), '');
	assert.equal(runningLabel(NO_TARGET, null), '');
	// Once a listing arrived, a computer it lacks reads like no choice.
	assert.equal(normalizeTarget('laptop-id', spaces(studio)), NO_TARGET);
	// An offline computer is still listed, so the choice stays and is refused by name.
	assert.equal(normalizeTarget('den-id', spaces(studio, den)), 'den-id');
	assert.equal(requestTarget(normalizeTarget('den-id', spaces(studio, den))), 'den-id');
});

test('the summary says what the tools will act on', () => {
	// Nothing chosen: the only connected desktop is what the tools use.
	assert.deepEqual(targetSummary(NO_TARGET, spaces(studio, den)), {
		name: 'Studio Mac',
		detail: 'Only connected desktop',
		status: 'online',
		ambiguous: false,
	});
	// Nothing chosen, several connected: the tools will ask.
	assert.deepEqual(targetSummary(NO_TARGET, spaces(studio, laptop)), {
		name: 'Choose a computer',
		detail: '2 desktops connected',
		status: 'ambiguous',
		ambiguous: true,
	});
	// Nothing chosen, none connected.
	assert.deepEqual(targetSummary(NO_TARGET, spaces(den)), {
		name: 'No desktop connected',
		detail: 'Open the desktop app to connect one',
		status: 'offline',
		ambiguous: false,
	});
	assert.deepEqual(targetSummary('server-home', spaces(studio)), {
		name: 'Luna’s home',
		detail: 'This server',
		status: 'online',
		ambiguous: false,
	});
	assert.deepEqual(targetSummary('den-id', spaces(studio, den)), {
		name: 'den',
		detail: 'Offline',
		status: 'offline',
		ambiguous: false,
	});
	assert.deepEqual(targetSummary('kiosk-id', spaces(kiosk)), {
		name: 'kiosk',
		detail: 'Needs permission',
		status: 'restricted',
		ambiguous: false,
	});
	// A choice the listing no longer has reads like no choice.
	assert.equal(targetSummary('forgotten-id', spaces(studio)).name, 'Studio Mac');
});

test('the request carries the stable id or nothing', () => {
	assert.equal(requestTarget(NO_TARGET), null);
	assert.equal(requestTarget('studio-id'), 'studio-id');
	assert.equal(requestTarget('server-home'), 'server-home');
	assert.equal(requestTarget(null), null);
});

test('the running label names the computer only when one is certain', () => {
	assert.equal(runningLabel('studio-id', spaces(studio, laptop)), 'on Studio Mac');
	assert.equal(runningLabel(NO_TARGET, spaces(studio, den)), 'on Studio Mac');
	assert.equal(runningLabel(NO_TARGET, spaces(studio, laptop)), '');
	assert.equal(runningLabel(NO_TARGET, spaces(den)), '');
	assert.equal(runningLabel('server-home', spaces(studio)), 'at home');
});

test('the choice is remembered per conversation', () => {
	assert.equal(targetStorageKey('companion', 'default'), 'nolune:target:companion/default');
	assert.notEqual(targetStorageKey('companion', 'a'), targetStorageKey('companion', 'b'));
});
