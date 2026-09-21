import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { formatPairingCodeInput, isCompletePairingCode, pairingErrorText } from '../src/lib/api/pairing.js';

test('pairing input is normalised to 1234-5678 as it is typed or pasted', () => {
	assert.equal(formatPairingCodeInput(''), '');
	assert.equal(formatPairingCodeInput('12'), '12');
	assert.equal(formatPairingCodeInput('1234'), '1234');
	assert.equal(formatPairingCodeInput('12345'), '1234-5');
	assert.equal(formatPairingCodeInput('1234-5678'), '1234-5678');
	assert.equal(formatPairingCodeInput(' 1234 5678 '), '1234-5678');
	assert.equal(formatPairingCodeInput('Pairing code:  4821-9037'), '4821-9037');
	assert.equal(formatPairingCodeInput('123456789'), '1234-5678', 'extra digits are dropped');
	assert.equal(formatPairingCodeInput(undefined), '');
});

test('completeness needs exactly eight digits', () => {
	assert.equal(isCompletePairingCode('1234-5678'), true);
	assert.equal(isCompletePairingCode('1234-567'), false);
	assert.equal(isCompletePairingCode('abcd-efgh'), false);
});

test('every server reason has a sentence and unknown reasons are honest', () => {
	for (const reason of ['invalid_code', 'rate_limited', 'cross_origin', 'auth_disabled']) {
		assert.ok(pairingErrorText(reason).length > 20, reason);
	}
	assert.match(pairingErrorText('rate_limited'), /10 minutes/);
	assert.match(pairingErrorText('invalid_code'), /5 minutes/);
	assert.match(pairingErrorText('unknown'), /server/i);
	assert.doesNotMatch(pairingErrorText('unknown'), /token/i, 'never tells a browser user to find the API token');
});

test('the pairing hint points at Settings → Connections, where the button lives (#37 follow-up)', () => {
	const gate = readFileSync(new URL('../src/lib/components/auth/AuthGate.svelte', import.meta.url), 'utf8');
	assert.match(gate, /Settings → Connections → Pair another browser/);
	assert.doesNotMatch(gate, /Settings → Server/, 'there is no Server settings page with a pairing button');
	const connections = readFileSync(new URL('../src/routes/[slug]/settings/connections/+page.svelte', import.meta.url), 'utf8');
	assert.match(connections, /Pair another browser/, 'the hint must name the page that actually has the button');
});
