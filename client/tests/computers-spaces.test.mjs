import test from 'node:test';
import assert from 'node:assert/strict';
import {
	HOME_SPACE_ID,
	STALE_HEARTBEAT_SECS,
	applyMachineEvent,
	buildSpaces,
	capabilitySummary,
	cuaLabel,
	deriveHealth,
	homeSpace,
	lastSeenLabel,
	permissionRows,
	platformLabel,
	sortSpaces,
	spaceHints,
	spaceStatus,
	spaceView,
	stateLabel,
} from '../src/lib/computers/spaces.js';

const NOW = 1_800_000_000;
const slug = 'companion';

/** A desktop as `GET /machines` lists it after #179; overrides pick the case. */
function desktop(overrides = {}) {
	return {
		machine_id: '8d3c2f4e-1111-4a2b-9c3d-000000000001',
		display_name: 'studio',
		custom_name: null,
		hostname: 'studio',
		os: 'macOS 15.1',
		platform: 'macos',
		location: 'desktop',
		screen_width: 2560,
		screen_height: 1440,
		permissions: { accessibility: 'granted', screen_capture: 'granted' },
		capabilities: ['screenshot', 'left_click', 'type', 'bash'],
		first_seen: NOW - 86400,
		last_seen: NOW - 5,
		instance_slug: slug,
		online: true,
		health: 'healthy',
		driver_version: null,
		cua_health: null,
		...overrides,
	};
}

const offline = desktop({ machine_id: 'off-1', hostname: 'laptop', display_name: 'laptop', online: false, health: 'unavailable', last_seen: NOW - 7200 });
const stale = desktop({ machine_id: 'stale-1', hostname: 'den', display_name: 'den', last_seen: NOW - STALE_HEARTBEAT_SECS - 5, health: 'degraded' });
const denied = desktop({ machine_id: 'denied-1', hostname: 'kiosk', display_name: 'kiosk', permissions: { accessibility: 'denied', screen_capture: 'granted' } });

test('health is re-derived from last_seen with the server threshold, never better than the server said', () => {
	assert.equal(STALE_HEARTBEAT_SECS, 45, 'mirrors STALE_HEARTBEAT_SECS in server/src/domain/machine.rs');
	assert.equal(deriveHealth(desktop(), NOW), 'healthy');
	assert.equal(deriveHealth(desktop(), NOW + STALE_HEARTBEAT_SECS - 5), 'healthy', 'a heartbeat inside the window is healthy');
	assert.equal(deriveHealth(desktop(), NOW + STALE_HEARTBEAT_SECS + 1), 'degraded', 'the clock alone degrades a silent heartbeat');
	assert.equal(deriveHealth(desktop({ health: 'degraded' }), NOW), 'degraded', "the server's degraded is kept while last_seen still looks fresh");
	assert.equal(deriveHealth(offline, NOW), 'unavailable', 'offline is unavailable whatever the timestamps say');
	assert.equal(deriveHealth(desktop({ online: false, health: 'healthy', last_seen: NOW }), NOW), 'unavailable');
});

test('a space is online, not responding, needing permission or offline, in that priority', () => {
	assert.equal(spaceStatus(desktop(), NOW), 'online');
	assert.equal(stateLabel(desktop(), NOW), 'Online');
	assert.equal(spaceStatus(stale, NOW), 'unhealthy');
	assert.equal(stateLabel(stale, NOW), 'Not responding');
	assert.equal(spaceStatus(denied, NOW), 'restricted');
	assert.equal(stateLabel(denied, NOW), 'Needs permission');
	assert.equal(spaceStatus(desktop({ permissions: { accessibility: 'prompt_required', screen_capture: 'granted' } }), NOW), 'restricted', 'a permission the user has not been asked for blocks too');
	assert.equal(spaceStatus(desktop({ permissions: { accessibility: 'unavailable', screen_capture: 'unavailable' } }), NOW), 'online', 'a platform that cannot report permissions is not restricted');
	assert.equal(spaceStatus(desktop({ permissions: null }), NOW), 'online', 'an older desktop app that reports nothing is not restricted');
	assert.equal(spaceStatus(offline, NOW), 'offline');
	assert.equal(stateLabel(offline, NOW), 'Offline');
	assert.equal(spaceStatus(desktop({ ...denied, last_seen: NOW - 100 }), NOW), 'unhealthy', 'not responding beats needing permission');
	assert.equal(spaceStatus(desktop({ ...denied, online: false }), NOW), 'offline', 'offline beats everything');
});

test('platform and last-seen labels read as sentences', () => {
	assert.equal(platformLabel(desktop()), 'macOS');
	assert.equal(platformLabel(desktop({ platform: 'windows', os: 'Windows 11' })), 'Windows');
	assert.equal(platformLabel(desktop({ platform: 'linux', os: 'Ubuntu 24.04' })), 'Linux');
	assert.equal(platformLabel(desktop({ platform: null, os: 'Darwin 24.1' })), 'macOS', 'the OS label is read when the protocol did not name the platform');
	assert.equal(platformLabel(desktop({ platform: null, os: 'Haiku R1' })), 'Haiku R1', 'an unknown OS keeps its own label');
	assert.equal(platformLabel(desktop({ platform: null, os: '' })), 'Unknown platform');

	assert.equal(lastSeenLabel(desktop(), NOW), 'Online now');
	assert.equal(lastSeenLabel(stale, NOW), `Last heartbeat ${STALE_HEARTBEAT_SECS + 5} s ago`);
	assert.equal(lastSeenLabel(desktop({ online: false, last_seen: NOW - 30 }), NOW), 'Seen just now');
	assert.equal(lastSeenLabel(desktop({ online: false, last_seen: NOW - 300 }), NOW), 'Seen 5 min ago');
	assert.equal(lastSeenLabel(offline, NOW), 'Seen 2 h ago');
	assert.equal(lastSeenLabel(desktop({ online: false, last_seen: NOW - 3 * 86400 }), NOW), 'Seen 3 days ago');
	assert.equal(lastSeenLabel(desktop({ online: false, last_seen: NOW + 60 }), NOW), 'Seen just now', 'a clock ahead of ours never reads as the future');
});

test('permissions, capabilities and the Cua driver are summarized without guessing', () => {
	assert.deepEqual(
		permissionRows(denied).map((p) => [p.key, p.label, p.state, p.stateLabel, p.blocking]),
		[
			['accessibility', 'Accessibility', 'denied', 'denied', true],
			['screen_capture', 'Screen recording', 'granted', 'granted', false],
		],
	);
	const askFirst = permissionRows(desktop({ permissions: { accessibility: 'prompt_required', screen_capture: 'unavailable' } }));
	assert.deepEqual(askFirst.map((p) => [p.stateLabel, p.blocking]), [['not asked yet', true], ['not available here', false]]);
	assert.deepEqual(permissionRows(desktop({ permissions: null })), [], 'nothing reported means no rows, not four unknowns');

	assert.equal(capabilitySummary(desktop()), '4 actions');
	assert.equal(capabilitySummary(desktop({ capabilities: ['screenshot'] })), '1 action');
	assert.equal(capabilitySummary(desktop({ capabilities: [] })), 'No actions reported');

	assert.equal(cuaLabel(desktop()), 'Cua driver not reported');
	assert.equal(cuaLabel(desktop({ driver_version: '0.28.2', cua_health: 'healthy' })), 'Cua driver 0.28.2 · healthy');
	assert.equal(cuaLabel(desktop({ driver_version: '0.28.2', cua_health: null })), 'Cua driver 0.28.2');
});

test('hints name the computer and the one thing to do about it', () => {
	assert.deepEqual(spaceHints(desktop(), NOW), [
		{ level: 'info', text: 'Screen actions use the desktop app’s built-in path until the Cua driver ships.' },
	]);
	assert.deepEqual(spaceHints(offline, NOW), [
		{ level: 'warn', text: 'laptop is offline. Open the Nolune desktop app there to reconnect it.' },
		{ level: 'info', text: 'Screen actions use the desktop app’s built-in path until the Cua driver ships.' },
	]);
	assert.deepEqual(spaceHints(stale, NOW)[0], {
		level: 'warn',
		text: `den has not answered for ${STALE_HEARTBEAT_SECS + 5} s. Check that the computer is awake and the desktop app is still running.`,
	});
	assert.deepEqual(spaceHints(denied, NOW)[0], {
		level: 'warn',
		text: 'Accessibility is denied on kiosk. Grant it to the Nolune desktop app in System Settings, then reconnect.',
	});
	assert.deepEqual(spaceHints(desktop({ permissions: { accessibility: 'granted', screen_capture: 'prompt_required' } }), NOW)[0], {
		level: 'warn',
		text: 'Screen recording has not been allowed on studio yet. Open the Nolune desktop app there to allow it.',
	});
	const renamed = desktop({ ...denied, custom_name: 'Front desk', display_name: 'Front desk' });
	assert.match(spaceHints(renamed, NOW)[0].text, /^Accessibility is denied on Front desk\./, 'hints use the name the user gave');
	assert.deepEqual(spaceHints(desktop({ capabilities: [] }), NOW)[0], {
		level: 'warn',
		text: 'studio reported no actions it can perform. Update the Nolune desktop app there.',
	});
	assert.deepEqual(spaceHints(desktop({ driver_version: '0.28.2', cua_health: 'healthy' }), NOW), [], 'a reported driver needs no hint');
	assert.deepEqual(spaceHints(desktop({ driver_version: '0.28.2', cua_health: 'degraded' }), NOW), [
		{ level: 'warn', text: 'The Cua driver on studio is degraded. Restart the Nolune desktop app there.' },
	]);
});

test('the server home is one row, first, and never a separate companion', () => {
	const home = homeSpace({ connected: true, address: 'nolune.local:26559', version: '0.36.0', companionName: 'Luna', nowSeconds: NOW });
	assert.equal(home.machine_id, HOME_SPACE_ID);
	assert.equal(home.location, 'server_local');
	assert.equal(home.online, true);
	assert.equal(deriveHealth(home, NOW), 'healthy');
	const view = spaceView(home, NOW);
	assert.equal(view.kind, 'home');
	assert.equal(view.name, 'Luna’s home');
	assert.equal(view.stateLabel, 'Online');
	assert.equal(view.location, 'This server');
	assert.equal(view.meta, 'This server · nolune.local:26559 · v0.36.0');
	assert.equal(view.note, 'Where Luna runs. The computers below are other places it can act; they are not separate companions.');
	assert.equal(view.canRename, false, 'the home is named after the companion, not renamed here');
	assert.deepEqual(view.permissions, []);
	assert.deepEqual(view.hints, []);

	const unnamed = spaceView(homeSpace({ connected: true, nowSeconds: NOW }), NOW);
	assert.equal(unnamed.name, 'Nolune’s home');
	assert.equal(unnamed.meta, 'This server');

	const away = spaceView(homeSpace({ connected: false, companionName: 'Luna', nowSeconds: NOW }), NOW);
	assert.equal(away.status, 'offline');
	assert.equal(away.stateLabel, 'Reconnecting', 'a closed browser socket is not the server being offline');
	assert.deepEqual(away.hints, [{ level: 'warn', text: 'This browser lost its connection to the server. The list updates as soon as it is back.' }]);
});

test('spaces sort home first, then online computers by name, then offline by last seen', () => {
	const home = homeSpace({ connected: true, nowSeconds: NOW });
	const zed = desktop({ machine_id: 'z', hostname: 'zed', display_name: 'zed' });
	const amy = desktop({ machine_id: 'a', hostname: 'amy', display_name: 'Amy' });
	const older = desktop({ machine_id: 'older', hostname: 'older', display_name: 'older', online: false, health: 'unavailable', last_seen: NOW - 90000 });
	const sorted = sortSpaces([older, zed, offline, stale, home, amy], NOW);
	assert.deepEqual(
		sorted.map((m) => m.machine_id),
		[HOME_SPACE_ID, 'a', 'stale-1', 'z', 'off-1', 'older'],
	);
	const listed = desktop({ machine_id: 'srv', hostname: 'srv', display_name: 'srv', location: 'server_local' });
	assert.equal(sortSpaces([zed, listed], NOW)[0].machine_id, 'srv', 'a server-local record from the listing is the home row');
});

test('buildSpaces synthesizes the home only when the listing has no server-local record', () => {
	const home = homeSpace({ connected: true, companionName: 'Luna', nowSeconds: NOW });
	const views = buildSpaces([offline, desktop()], NOW, home);
	assert.deepEqual(views.map((v) => [v.id, v.kind, v.stateLabel]), [
		[HOME_SPACE_ID, 'home', 'Online'],
		[desktop().machine_id, 'desktop', 'Online'],
		['off-1', 'desktop', 'Offline'],
	]);
	const listed = desktop({ machine_id: 'srv', hostname: 'srv', display_name: 'srv', location: 'server_local', capabilities: ['bash'] });
	const withListed = buildSpaces([listed, desktop()], NOW, home);
	assert.deepEqual(withListed.map((v) => v.id), ['srv', desktop().machine_id], 'the listed home replaces the synthesized one');
	assert.equal(withListed[0].kind, 'home');
	assert.equal(withListed[0].capabilities, '1 action', 'a listed home keeps its own facts');
	assert.deepEqual(buildSpaces([], NOW, null).map((v) => v.id), [], 'no home is invented when the caller has none');
});

test('a desktop view carries every fact the surface shows', () => {
	const view = spaceView(desktop({ custom_name: 'Studio Mac', display_name: 'Studio Mac' }), NOW);
	assert.equal(view.id, desktop().machine_id);
	assert.equal(view.kind, 'desktop');
	assert.equal(view.name, 'Studio Mac');
	assert.equal(view.customName, 'Studio Mac');
	assert.equal(view.hostname, 'studio');
	assert.equal(view.status, 'online');
	assert.equal(view.health, 'healthy');
	assert.equal(view.meta, 'macOS · 2560×1440 · Desktop');
	assert.equal(view.lastSeen, 'Online now');
	assert.equal(view.capabilities, '4 actions');
	assert.equal(view.cua, 'Cua driver not reported');
	assert.equal(view.canRename, true);
	assert.equal(view.note, '');
	assert.equal(spaceView(desktop({ screen_width: 0, screen_height: 0 }), NOW).meta, 'macOS · Desktop', 'an unreported screen is left out');
});

test('machine events upsert one row by stable id and forget drops it; others are ignored', () => {
	const start = [desktop(), offline];
	const renamed = desktop({ custom_name: 'Studio Mac', display_name: 'Studio Mac' });
	const afterRename = applyMachineEvent(start, { type: 'machine_updated', instance_slug: slug, machine: renamed });
	assert.deepEqual(afterRename.map((m) => m.display_name), ['Studio Mac', 'laptop'], 'the same id is replaced in place');
	const newcomer = desktop({ machine_id: 'new-1', hostname: 'new', display_name: 'new' });
	const afterJoin = applyMachineEvent(afterRename, { type: 'machine_updated', instance_slug: slug, machine: newcomer });
	assert.equal(afterJoin.length, 3);
	const afterForget = applyMachineEvent(afterJoin, { type: 'machine_forgotten', instance_slug: slug, machine_id: 'off-1' });
	assert.deepEqual(afterForget.map((m) => m.machine_id), [desktop().machine_id, 'new-1']);
	assert.equal(applyMachineEvent(afterForget, { type: 'mood_updated', instance_slug: slug, mood: 'calm' }), afterForget, 'an unrelated event returns the same array');
	assert.equal(applyMachineEvent(afterForget, { type: 'machine_forgotten', instance_slug: slug, machine_id: 'nobody' }), afterForget, 'forgetting an unknown id changes nothing');
	assert.deepEqual(start.map((m) => m.display_name), ['studio', 'laptop'], 'the input is never mutated');
});
