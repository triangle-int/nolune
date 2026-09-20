import test from 'node:test';
import assert from 'node:assert/strict';
import {
	acceptErrorText,
	inviteHandoff,
	inviteView,
	lastSeenLabel,
	looksLikeUrl,
	overviewView,
	peerView,
	rotationSummary,
	shortId,
	sortPeers,
} from '../src/lib/federation/companions.js';

const NOW = 1_800_000_000;
const SECRET = 'issue-108-secret-that-must-never-be-rendered';
const LINE = 'nolune-invite-v1.eyJvcmlnaW4iOiJodHRwczovL2EudGVzdCJ9';

function document(id = 'TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E') {
	return { version: 1, companion_id: id, public_key: 'pk-' + id, created_at: NOW - 86_400, signature: 'sig' };
}

/** A peer as `GET /api/federation/peers` lists it; overrides pick the case. */
function peer(overrides = {}) {
	return {
		companion_id: 'TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E',
		public_key: 'SN7Fvp7FYlYfvTUUW4vPFzy1jh9gtfDVlA7I3_QSj4U',
		state: 'paired',
		role: 'issuer',
		pairing_id: '9f1c0b7e2a6d4c31',
		approved_origins: ['https://molinka.example'],
		created_at: NOW - 3_600,
		updated_at: NOW - 1_800,
		last_seen_at: NOW - 120,
		rotation_history: [],
		...overrides,
	};
}

test('a paired peer reads as paired with its origins and last sighting', () => {
	const view = peerView(peer(), NOW);
	assert.equal(view.id, 'TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E');
	assert.equal(view.shortId, shortId('TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E'));
	assert.match(view.shortId, /^TFccHElq…cQ7E$/);
	assert.equal(view.state, 'paired');
	assert.equal(view.stateLabel, 'Paired');
	assert.equal(view.tone, 'online');
	assert.equal(view.roleLabel, 'You invited it');
	assert.deepEqual(view.approvedOrigins, ['https://molinka.example']);
	assert.equal(view.pendingOrigin, null);
	assert.equal(view.lastSeen, 'last seen 2 min ago');
	assert.equal(view.canConfirm, false);
	assert.equal(view.canRevoke, true);
	assert.equal(view.rotations, 0);
	assert.equal(view.note, '');
});

test('a pending peer says whose turn it is and only the issuer can confirm', () => {
	const mine = peerView(peer({ state: 'pending', role: 'issuer', approved_origins: [], pending_origin: 'http://localhost:26702' }), NOW);
	assert.equal(mine.stateLabel, 'Waiting for you');
	assert.equal(mine.tone, 'warn');
	assert.equal(mine.canConfirm, true);
	assert.equal(mine.canRevoke, true);
	assert.deepEqual(mine.approvedOrigins, []);
	assert.equal(mine.pendingOrigin, 'http://localhost:26702');
	assert.match(mine.note, /Confirm to pair it; it will be reached at http:\/\/localhost:26702\./);

	const theirs = peerView(peer({ state: 'pending', role: 'accepter' }), NOW);
	assert.equal(theirs.stateLabel, 'Waiting for its owner');
	assert.equal(theirs.tone, 'warn');
	assert.equal(theirs.canConfirm, false, 'the accepter cannot confirm');
	assert.equal(theirs.canRevoke, true);
	assert.equal(theirs.roleLabel, 'It invited you');
	assert.match(theirs.note, /its owner still has to confirm/);
});

test('a revoked peer is muted, cannot be confirmed or revoked again, and says how to pair again', () => {
	const view = peerView(peer({ state: 'revoked', approved_origins: [], last_seen_at: NOW - 86_400 * 3 }), NOW);
	assert.equal(view.stateLabel, 'Revoked');
	assert.equal(view.tone, 'muted');
	assert.equal(view.canConfirm, false);
	assert.equal(view.canRevoke, false);
	assert.equal(view.lastSeen, 'last seen 3 days ago');
	assert.match(view.note, /new invite/);
});

test('a peer never seen has no sighting and a rotated one keeps its previous ids', () => {
	const never = peerView(peer({ last_seen_at: undefined }), NOW);
	assert.equal(never.lastSeen, 'never seen');
	const rotated = peerView(
		peer({
			companion_id: 'new-id',
			rotation_history: [
				{ rotation: { version: 1, previous: document('old-id'), identity: document('new-id'), rotated_at: NOW - 600, endorsement: 'e', signature: 's' }, accepted_at: NOW - 590 },
			],
		}),
		NOW,
	);
	assert.equal(rotated.rotations, 1);
	assert.deepEqual(rotated.previousIds, ['old-id']);
	assert.match(rotated.note, /rotated its key once/);
	assert.match(rotated.note, /old-id/);
});

test('last-seen labels are words at every distance', () => {
	assert.equal(lastSeenLabel(NOW, NOW), 'last seen just now');
	assert.equal(lastSeenLabel(NOW - 60, NOW), 'last seen just now');
	assert.equal(lastSeenLabel(NOW - 600, NOW), 'last seen 10 min ago');
	assert.equal(lastSeenLabel(NOW - 7_200, NOW), 'last seen 2 h ago');
	assert.equal(lastSeenLabel(NOW - 86_400 * 5, NOW), 'last seen 5 days ago');
	assert.equal(lastSeenLabel(undefined, NOW), 'never seen');
	assert.equal(lastSeenLabel(null, NOW), 'never seen');
	assert.equal(lastSeenLabel(NOW + 500, NOW), 'last seen just now', 'a clock ahead of ours is not the future');
});

test('peers sort paired first, then pending, then revoked, freshest first within each', () => {
	const sorted = sortPeers([
		peer({ companion_id: 'revoked-old', state: 'revoked', last_seen_at: NOW - 9_000 }),
		peer({ companion_id: 'pending', state: 'pending', last_seen_at: NOW - 10 }),
		peer({ companion_id: 'paired-old', last_seen_at: NOW - 5_000 }),
		peer({ companion_id: 'paired-never', last_seen_at: undefined }),
		peer({ companion_id: 'paired-new', last_seen_at: NOW - 5 }),
	]);
	assert.deepEqual(
		sorted.map((p) => p.companion_id),
		['paired-new', 'paired-old', 'paired-never', 'pending', 'revoked-old'],
	);
});

test('an invite counts down and says when it has expired, without its secret', () => {
	const open = inviteView({ id: '9f1c0b7e2a6d4c31', state: 'invited', created_at: NOW - 60, expires_at: NOW + 540 }, NOW);
	assert.equal(open.id, '9f1c0b7e2a6d4c31');
	assert.equal(open.countdown, '9:00');
	assert.equal(open.expired, false);
	assert.match(open.text, /expires in 9:00/);
	const gone = inviteView({ id: 'x', state: 'invited', created_at: NOW - 900, expires_at: NOW - 1 }, NOW);
	assert.equal(gone.countdown, '0:00');
	assert.equal(gone.expired, true);
	assert.match(gone.text, /expired/);
	assert.ok(!('secret' in open) && !('invite' in open), 'an invite view never carries a secret');
});

test('the handoff panel keeps the one line and the clock, nothing else from the response', () => {
	const issued = {
		id: '9f1c0b7e2a6d4c31',
		secret: SECRET,
		invite: LINE,
		created_at: NOW,
		expires_at: NOW + 600,
		expires_in_secs: 600,
		origin: 'https://a.test',
		issuer: document(),
	};
	const handoff = inviteHandoff(issued);
	assert.deepEqual(Object.keys(handoff).sort(), ['expiresAt', 'id', 'line', 'origin']);
	assert.equal(handoff.line, LINE);
	assert.equal(handoff.expiresAt, NOW + 600);
	assert.equal(handoff.origin, 'https://a.test');
	assert.ok(!JSON.stringify(handoff).includes(SECRET), 'the raw secret never leaves the response');
	assert.ok(!LINE.includes('://'), 'the line is not a URL');
});

test('the overview view carries ids, keys, counts, and views, never a secret', () => {
	const overview = overviewView(
		{
			companion_id: 'me',
			identity: document('me'),
			rotations: [{ version: 1, previous: document('old-me'), identity: document('me'), rotated_at: NOW - 100, endorsement: 'e', signature: 's' }],
			invites: [{ id: 'inv', state: 'invited', created_at: NOW - 60, expires_at: NOW + 540 }],
			peers: [peer({ companion_id: 'p1' }), peer({ companion_id: 'p2', state: 'pending' })],
		},
		NOW,
	);
	assert.equal(overview.companionId, 'me');
	assert.equal(overview.publicKey, 'pk-me');
	assert.equal(overview.rotations, 1);
	assert.deepEqual(overview.previousIds, ['old-me']);
	assert.equal(overview.invites.length, 1);
	assert.equal(overview.invites[0].countdown, '9:00');
	assert.deepEqual(overview.peers.map((p) => p.id), ['p1', 'p2']);
	assert.equal(overview.paired, 1);
	assert.equal(overview.waitingForYou, 0);
	const text = JSON.stringify(overview);
	assert.ok(!text.includes('secret') && !text.includes('nolune-invite'), text);
});

test('URL-shaped input is refused before it is sent, and an invite line is not URL-shaped', () => {
	for (const url of ['https://a.test/?invite=abc', 'http://localhost:26559/federation/v1/pair?secret=x', 'HTTPS://A.TEST', 'nolune://x']) {
		assert.equal(looksLikeUrl(url), true, url);
	}
	assert.equal(looksLikeUrl(LINE), false);
	assert.equal(looksLikeUrl('  ' + LINE + '\n'), false);
	assert.equal(looksLikeUrl(''), false);
	assert.equal(looksLikeUrl('http'), false);
});

test('every server refusal has a sentence for the accepting owner and none echoes the line', () => {
	const cases = {
		invalid_invite: /already been used|expired|wrong/i,
		malformed: /not an invite/i,
		invalid_body: /not an invite/i,
		peer_unreachable: /could not be reached/i,
		peer_refused: /refused/i,
		issuer_mismatch: /this companion|itself/i,
		federation_unavailable: /server/i,
		invalid_origin: /address/i,
	};
	for (const [code, expected] of Object.entries(cases)) {
		const text = acceptErrorText({ code, message: 'server says ' + SECRET, peerError: code === 'peer_refused' ? 'invalid_invite' : undefined });
		assert.match(text, expected, code);
		assert.ok(!text.includes(SECRET), `${code} echoed the server message`);
	}
	assert.match(acceptErrorText({ code: 'peer_refused', peerError: 'invalid_invite' }), /used|expired|wrong/i);
	assert.match(acceptErrorText({ code: 'something_new' }), /could not accept/i);
	assert.match(acceptErrorText(new Error('network down')), /could not accept/i);
	assert.match(acceptErrorText(null), /could not accept/i);
});

test('a rotation report names the new id, who was told, and who still needs the proof', () => {
	const report = {
		identity: document('new-me'),
		rotation: { version: 1, previous: document('me'), identity: document('new-me'), rotated_at: NOW, endorsement: 'e', signature: 's' },
		notified: ['p1', 'p2'],
		unreachable: ['p3'],
	};
	const text = rotationSummary(report);
	assert.match(text, /new-me/);
	assert.match(text, /2 companions were told/);
	assert.match(text, /p3/);
	assert.match(text, /could not be reached/);
	assert.match(rotationSummary({ ...report, notified: ['p1'], unreachable: [] }), /1 companion was told/);
	assert.match(rotationSummary({ ...report, notified: [], unreachable: [] }), /No companions/i);
});
