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
	reconcileListing,
	sortSpaces,
	spaceHints,
	spaceStatus,
	spaceView,
	stateLabel,
} from '../src/lib/computers/spaces.js';

const NOW = 1_800_000_000;
const slug = 'companion';

/**
 * A desktop as `GET /machines` lists it after #179 and #19: the toolcalls
 * the desktop app executes, and no driver (so no grants) unless an
 * override adds one; the grants on a row are always the Cua driver's.
 */
function desktop(overrides = {}) {
	return {
		machine_id: '8d3c2f4e-1111-4a2b-9c3d-000000000001',
		display_name: 'studio',
		custom_name: null,
		hostname: 'studio',
		os: 'macOS 15.1',
		platform: 'macos',
		location: 'desktop',
		permissions: null,
		capabilities: ['bash', 'file_read', 'file_write', 'file_list', 'upload_file'],
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

/** The same desktop with a Cua driver holding `permissions`. */
function withDriver(permissions = { accessibility: 'granted', screen_capture: 'granted' }, overrides = {}) {
	return desktop({ driver_version: '0.28.2', cua_health: 'healthy', permissions, ...overrides });
}

const offline = desktop({ machine_id: 'off-1', hostname: 'laptop', display_name: 'laptop', online: false, health: 'unavailable', last_seen: NOW - 7200 });
const stale = desktop({ machine_id: 'stale-1', hostname: 'den', display_name: 'den', last_seen: NOW - STALE_HEARTBEAT_SECS - 5, health: 'degraded' });
const denied = withDriver({ accessibility: 'denied', screen_capture: 'granted' }, { machine_id: 'denied-1', hostname: 'kiosk', display_name: 'kiosk' });

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
	assert.equal(spaceStatus(desktop(), NOW), 'online', 'a desktop without a driver has no grants to need (#19)');
	assert.equal(stateLabel(desktop(), NOW), 'Online');
	assert.equal(spaceStatus(withDriver(), NOW), 'online');
	assert.equal(spaceStatus(stale, NOW), 'unhealthy');
	assert.equal(stateLabel(stale, NOW), 'Not responding');
	assert.equal(spaceStatus(denied, NOW), 'restricted', "the Cua driver's grants decide");
	assert.equal(stateLabel(denied, NOW), 'Needs permission');
	assert.equal(spaceStatus(withDriver({ accessibility: 'prompt_required', screen_capture: 'granted' }), NOW), 'restricted', 'a grant the driver has not been asked for blocks too');
	assert.equal(spaceStatus(withDriver({ accessibility: 'unavailable', screen_capture: 'unavailable' }), NOW), 'online', 'a platform that cannot report permissions is not restricted');
	assert.equal(spaceStatus(desktop({ permissions: null }), NOW), 'online', 'no driver, no grants, not restricted');
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
	const askFirst = permissionRows(withDriver({ accessibility: 'prompt_required', screen_capture: 'unavailable' }));
	assert.deepEqual(askFirst.map((p) => [p.stateLabel, p.blocking]), [['not asked yet', true], ['not available here', false]]);
	assert.deepEqual(permissionRows(desktop()), [], 'no driver means no rows, not four unknowns: the app has no grants of its own to show (#19)');

	assert.equal(capabilitySummary(desktop()), '5 actions');
	assert.equal(capabilitySummary(desktop({ capabilities: ['bash'] })), '1 action');
	assert.equal(capabilitySummary(desktop({ capabilities: [] })), 'No actions reported');

	assert.equal(cuaLabel(desktop()), 'Cua driver not reported');
	assert.equal(cuaLabel(desktop({ driver_version: '0.28.2', cua_health: 'healthy' })), 'Cua driver 0.28.2 · healthy');
	assert.equal(cuaLabel(desktop({ driver_version: '0.28.2', cua_health: null })), 'Cua driver 0.28.2');
});

test('hints name the computer and the one thing to do about it', () => {
	// A desktop without a driver has no screen path of its own (#19): the
	// hint says what still works there and the one thing to do about it.
	// That is a button in the desktop app, never a command (#231): a desktop
	// user has no `nolune` on their PATH.
	const noDriver = (name) =>
		`${name} has no Cua driver: the Nolune desktop app runs commands and files there, but it cannot see or act in windows. To give it one, open the Nolune desktop app there and press Install driver under Settings \u2192 Computer use.`;
	assert.deepEqual(spaceHints(desktop(), NOW), [{ level: 'info', text: noDriver('studio') }]);
	assert.deepEqual(spaceHints(offline, NOW), [
		{ level: 'warn', text: 'laptop is offline. Open the Nolune desktop app there to reconnect it.' },
		{ level: 'info', text: noDriver('laptop') },
	]);
	assert.deepEqual(
		spaceHints(desktop({ driver_version: '0.28.2', cua_health: 'healthy' }), NOW),
		[],
		'a desktop with a healthy driver needs nothing',
	);
	assert.deepEqual(spaceHints(stale, NOW)[0], {
		level: 'warn',
		text: `den has not answered for ${STALE_HEARTBEAT_SECS + 5} s. Check that the computer is awake and the desktop app is still running.`,
	});
	// The grants are the Cua driver's, and the one thing to do is to grant
	// the driver from the desktop app's Settings: never to grant the app.
	assert.deepEqual(spaceHints(denied, NOW), [
		{
			level: 'warn',
			text: "Accessibility is denied to the Cua driver on kiosk. To allow it, grant it from the Nolune desktop app's Settings there, then reconnect.",
		},
	]);
	assert.deepEqual(spaceHints(withDriver({ accessibility: 'granted', screen_capture: 'prompt_required' }), NOW), [
		{
			level: 'warn',
			text: "Screen recording has not been granted to the Cua driver on studio yet. To allow it, grant it from the Nolune desktop app's Settings there.",
		},
	]);
	for (const hint of [...spaceHints(denied, NOW), ...spaceHints(withDriver({ accessibility: 'prompt_required', screen_capture: 'prompt_required' }), NOW)]) {
		assert.doesNotMatch(hint.text, /Grant it to the Nolune desktop app/, 'nothing asks the user to grant the app itself');
	}
	const renamed = withDriver({ accessibility: 'denied', screen_capture: 'granted' }, { ...denied, custom_name: 'Front desk', display_name: 'Front desk' });
	assert.match(spaceHints(renamed, NOW)[0].text, /^Accessibility is denied to the Cua driver on Front desk\./, 'hints use the name the user gave');
	assert.deepEqual(spaceHints(withDriver(undefined, { capabilities: [] }), NOW), [
		{ level: 'warn', text: 'studio reported no actions it can perform. Update the Nolune desktop app there.' },
	]);
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
	const withListed = buildSpaces([listed, desktop()], NOW, home, 'Luna');
	assert.deepEqual(withListed.map((v) => v.id), ['srv', desktop().machine_id], 'the listed home replaces the synthesized one');
	assert.equal(withListed[0].kind, 'home');
	assert.equal(withListed[0].capabilities, '1 action', 'a listed home keeps its own facts');
	assert.equal(withListed[0].note, 'Where Luna runs. The computers below are other places it can act; they are not separate companions.', 'the note names the companion, not the record');
	assert.deepEqual(buildSpaces([], NOW, null).map((v) => v.id), [], 'no home is invented when the caller has none');
});

test('a listed server-local record is the home row but keeps its own state, facts and hints', () => {
	// Only the synthesized row stands for this browser's socket; a record the
	// server listed is a real machine (the Cua driver beside the server) and
	// reads like one: Offline, not Reconnecting, with its permissions and hints.
	const listedOffline = desktop({ machine_id: 'srv', hostname: 'srv', display_name: 'srv', location: 'server_local', online: false, health: 'unavailable', last_seen: NOW - 7200, driver_version: '0.28.2', cua_health: 'unavailable' });
	assert.equal(stateLabel(listedOffline, NOW), 'Offline');
	const offlineView = spaceView(listedOffline, NOW, 'Luna');
	assert.equal(offlineView.kind, 'home');
	assert.equal(offlineView.status, 'offline');
	assert.equal(offlineView.stateLabel, 'Offline');
	assert.equal(offlineView.location, 'This server');
	assert.equal(offlineView.meta, 'macOS · This server');
	assert.equal(offlineView.lastSeen, 'Seen 2 h ago');
	assert.equal(offlineView.note, 'Where Luna runs. The computers below are other places it can act; they are not separate companions.');
	assert.equal(offlineView.cua, 'Cua driver 0.28.2 · unavailable');
	assert.deepEqual(offlineView.hints, [
		{ level: 'warn', text: 'srv is offline. Start the Cua driver on the server to reconnect it.' },
		{ level: 'warn', text: 'The Cua driver on srv is unavailable. Restart it on the server.' },
	]);
	assert.equal(offlineView.canForget, true, 'an offline listed record can be forgotten like any other');

	const listedDenied = desktop({ machine_id: 'srv', hostname: 'srv', display_name: 'srv', location: 'server_local', permissions: { accessibility: 'denied', screen_capture: 'granted' }, capabilities: [], driver_version: '0.28.2', cua_health: 'healthy' });
	const deniedView = spaceView(listedDenied, NOW, 'Luna');
	assert.equal(deniedView.stateLabel, 'Needs permission');
	assert.equal(deniedView.status, 'restricted');
	assert.deepEqual(deniedView.permissions.map((p) => [p.key, p.state, p.blocking]), [
		['accessibility', 'denied', true],
		['screen_capture', 'granted', false],
	]);
	assert.equal(deniedView.capabilities, 'No actions reported');
	assert.deepEqual(deniedView.hints, [
		{ level: 'warn', text: 'Accessibility is denied to the Cua driver on srv. To allow it, run cua-driver permissions grant on the server, then reconnect.' },
		{ level: 'warn', text: 'srv reported no actions it can perform. Update the Cua driver on the server.' },
	]);
	assert.equal(deniedView.lastSeen, 'Online now');
	assert.equal(deniedView.canForget, false, 'a connected record cannot be forgotten');
	assert.equal(spaceView(listedDenied, NOW).note, 'Where Nolune runs. The computers below are other places it can act; they are not separate companions.', 'the product name stands in when no companion name is passed');

	const listedStale = desktop({ machine_id: 'srv', hostname: 'srv', display_name: 'srv', location: 'server_local', last_seen: NOW - STALE_HEARTBEAT_SECS - 5, health: 'degraded' });
	assert.deepEqual(
		spaceHints(listedStale, NOW),
		[{ level: 'warn', text: `srv has not answered for ${STALE_HEARTBEAT_SECS + 5} s. Check that the server is awake and the Cua driver is still running.` }],
		'a server-local record never gets the desktop-app info hint',
	);

	// The synthesized row keeps its own copy: its absence is this browser's socket.
	const away = spaceView(homeSpace({ connected: false, companionName: 'Luna', nowSeconds: NOW }), NOW);
	assert.equal(away.stateLabel, 'Reconnecting');
	assert.equal(away.canForget, false);
	assert.deepEqual(away.hints, [{ level: 'warn', text: 'This browser lost its connection to the server. The list updates as soon as it is back.' }]);
});

test('only an offline listed record can be forgotten', () => {
	assert.equal(spaceView(desktop(), NOW).canForget, false, 'the server refuses to forget a connected computer');
	assert.equal(spaceView(offline, NOW).canForget, true);
	assert.equal(spaceView(stale, NOW).canForget, false, 'a silent socket is still open');
	assert.equal(spaceView(homeSpace({ connected: false, nowSeconds: NOW }), NOW).canForget, false, 'the synthesized home is not a record');
});

test('a listing folds into rows changed since it was requested without reverting them', () => {
	// A poll that was in flight when an event or a rename landed must not
	// undo it: rows changed since the request started keep their local state.
	const renamed = desktop({ custom_name: 'Studio Mac', display_name: 'Studio Mac' });
	const newcomer = desktop({ machine_id: 'new-1', hostname: 'new', display_name: 'new' });
	const local = [renamed, newcomer];
	const listing = [desktop(), offline, desktop({ ...stale, last_seen: NOW - 1, health: 'healthy' })];
	const merged = reconcileListing(local, listing, ['off-1', desktop().machine_id, 'new-1']);
	assert.deepEqual(
		merged.map((m) => [m.machine_id, m.display_name]),
		[[desktop().machine_id, 'Studio Mac'], ['stale-1', 'den'], ['new-1', 'new']],
		'a renamed row keeps its name, a forgotten row stays gone, a newcomer stays, everything else takes the listing',
	);
	assert.equal(reconcileListing(local, listing, []), listing, 'nothing changed since the request means the listing as it came');
	assert.deepEqual(reconcileListing([], listing, ['nobody']).map((m) => m.machine_id), listing.map((m) => m.machine_id), 'an id that was neither listed nor kept changes nothing');
	assert.deepEqual(local.map((m) => m.display_name), ['Studio Mac', 'new'], 'the input is never mutated');
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
	assert.equal(view.meta, 'macOS · Desktop', 'platform and location only: no screen size is reported (#19)');
	assert.equal(view.lastSeen, 'Online now');
	assert.deepEqual(view.permissions, [], 'no driver: no grants to show, and none of the app\'s own (#19)');
	assert.equal(view.capabilities, '5 actions');
	assert.equal(view.cua, 'Cua driver not reported');
	assert.equal(view.canRename, true);
	assert.equal(view.note, '');

	const driven = spaceView(withDriver({ accessibility: 'granted', screen_capture: 'denied' }), NOW);
	assert.equal(driven.status, 'restricted');
	assert.deepEqual(driven.permissions.map((p) => [p.key, p.state, p.blocking]), [
		['accessibility', 'granted', false],
		['screen_capture', 'denied', true],
	], "the rows are the Cua driver's grants");
	assert.equal(driven.cua, 'Cua driver 0.28.2 · healthy');
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
