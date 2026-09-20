import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import {
	boundTextPath,
	canFlag,
	conflictIsPending,
	conflictPrompt,
	confidenceLabel,
	correctionOutcome,
	flagBadges,
	flagControls,
	isMediaMemory,
	mediaBoundText,
	memoryBody,
	reasonLabel,
	recalledWhen,
	recallsOf,
	receiptHeading,
	receiptSummary,
	receiptsByMessage,
	sourceLabel,
	sourceStatusCopy,
	uniqueMemories,
} from '../src/lib/memory/receipts.js';

const tea = {
	path: 'about/tea.md',
	source: 'about/tea.md',
	excerpt: 'likes oolong, brews it at 85°C',
	reason: 'semantic',
	confidence: 'high',
	retrieved_at: '2026-09-20T09:00:00Z',
	source_status: 'present',
};
const sky = {
	path: 'photos/sky.png',
	source: 'photos/sky.png.md',
	excerpt: 'sky over Lisbon at dusk',
	reason: 'linked_to',
	linked_from: 'about/tea.md',
	confidence: 'low',
	retrieved_at: '2026-09-20T08:00:00Z',
	source_status: 'missing',
};

test('reasons and confidence buckets are plain sentences, never scores', () => {
	assert.equal(receiptHeading('Luna'), 'Why did Luna remember this?');
	assert.equal(receiptHeading(''), 'Why did Nolune remember this?');
	assert.equal(reasonLabel(tea), 'Matches the meaning of the conversation');
	assert.equal(reasonLabel({ ...tea, reason: 'keyword' }), 'Contains words from the conversation');
	assert.equal(reasonLabel(sky), 'Linked from tea');
	assert.equal(reasonLabel({ ...sky, linked_from: undefined }), 'Linked from another recalled memory');
	assert.equal(reasonLabel({ ...tea, reason: 'matched' }), 'Matched the conversation');
	assert.equal(reasonLabel({ ...tea, reason: 'pinned' }), 'Pinned by you, recalled every turn');
	assert.equal(reasonLabel({ ...tea, reason: 'something_new' }), 'Recalled');
	assert.equal(confidenceLabel('high'), 'High confidence');
	assert.equal(confidenceLabel('medium'), 'Medium confidence');
	assert.equal(confidenceLabel('low'), 'Low confidence');
	assert.equal(confidenceLabel(undefined), 'Confidence not reported');
});

test('missing and media sources are stated honestly', () => {
	assert.equal(sourceStatusCopy(tea), '');
	assert.equal(sourceStatusCopy(sky), 'This memory was forgotten after this reply. The excerpt is what was recalled at the time.');
	assert.equal(isMediaMemory(tea), false);
	assert.equal(isMediaMemory(sky), true);
	assert.equal(sourceLabel(tea), 'about/tea.md');
	assert.equal(sourceLabel(sky), 'photos/sky.png · described in photos/sky.png.md');
	assert.equal(receiptSummary([]), 'No memories were used for this reply.');
	assert.equal(receiptSummary([tea]), '1 memory shaped this reply');
	assert.equal(receiptSummary([tea, sky]), '2 memories shaped this reply');
});

test('retrieval time is relative when recent and a date otherwise', () => {
	const now = Date.parse('2026-09-20T09:00:30Z');
	assert.equal(recalledWhen('2026-09-20T09:00:00Z', now), 'just now');
	assert.equal(recalledWhen('2026-09-20T08:55:00Z', now), '5m ago');
	assert.equal(recalledWhen('2026-09-20T06:00:00Z', now), '3h ago');
	assert.equal(recalledWhen('2026-09-18T09:00:00Z', now), '2d ago');
	assert.equal(recalledWhen('2026-09-01T09:00:00Z', now), '2026-09-01');
	assert.equal(recalledWhen('not a date', now), '');
});

test('receipts are keyed by message and never cross the chat they were loaded for', () => {
	const receipts = [
		{ message_id: 'msg_2', chat_id: 'default', memories: [tea] },
		{ message_id: 'msg_1', chat_id: 'default', memories: [] },
		{ message_id: 'msg_9', chat_id: 'chat_other', memories: [sky] },
	];
	const byMessage = receiptsByMessage(receipts, 'default');
	assert.deepEqual([...byMessage.keys()].sort(), ['msg_1', 'msg_2']);
	assert.deepEqual(byMessage.get('msg_1'), []);
	assert.equal(byMessage.get('msg_9'), undefined);
	assert.equal(receiptsByMessage(receipts, '').size, 0);

	const recalls = recallsOf(receipts, 'about/tea.md');
	assert.deepEqual(recalls.map((r) => [r.chat_id, r.message_id]), [['default', 'msg_2']]);
	assert.equal(recallsOf(receipts, 'photos/sky.png.md')[0].memory.path, 'photos/sky.png');
	const later = { message_id: 'msg_3', chat_id: 'default', memories: [{ ...tea, retrieved_at: '2026-09-21T09:00:00Z' }] };
	assert.deepEqual(recallsOf([...receipts, later], 'about/tea.md').map((r) => r.message_id), ['msg_3', 'msg_2']);
	assert.deepEqual(recallsOf(receipts, 'nowhere.md'), []);
});

test('a conflicting correction is put to the user as two statements to choose from', () => {
	const conflict = {
		conflict_id: 'corr_2_b',
		path: 'about/tea.md',
		current: { id: 'corr_1_a', statement: 'likes oolong', corrected_at: '2026-09-20T09:00:00Z' },
		proposed: { id: 'corr_2_b', statement: 'likes matcha', corrected_at: '2026-09-20T10:00:00Z' },
	};
	const prompt = conflictPrompt(conflict);
	assert.equal(prompt.conflictId, 'corr_2_b');
	assert.equal(prompt.question, 'Two of your corrections to tea disagree. Which one should stay?');
	assert.deepEqual(prompt.options.map((o) => o.keep), ['current', 'proposed']);
	assert.equal(prompt.options[0].title, 'Keep the current statement');
	assert.equal(prompt.options[0].statement, 'likes oolong');
	assert.equal(prompt.options[1].title, 'Use the new statement');
	assert.equal(prompt.options[1].statement, 'likes matcha');
	assert.equal(prompt.options[1].corrected_at, '2026-09-20T10:00:00Z');

	assert.deepEqual(correctionOutcome({ status: 'applied', path: 'about/tea.md', correction: {} }), { kind: 'applied' });
	assert.deepEqual(correctionOutcome({ status: 'unchanged', path: 'about/tea.md' }), { kind: 'unchanged' });
	const outcome = correctionOutcome({ status: 'needs_resolution', ...conflict });
	assert.equal(outcome.kind, 'needs_resolution');
	assert.equal(outcome.conflict.conflict_id, 'corr_2_b');
	assert.equal(correctionOutcome({ status: 'needs_resolution', path: 'about/tea.md' }).kind, 'error');
	assert.equal(correctionOutcome(null).kind, 'error');
});

test('the correction editor starts from the body, never the stamped frontmatter', () => {
	assert.equal(memoryBody('---\ncreated: 2026-09-20\nupdated: 2026-09-20\npinned: true\n---\nlikes oolong\n'), 'likes oolong');
	assert.equal(memoryBody('likes oolong'), 'likes oolong');
	assert.equal(memoryBody('---\nunterminated'), '---\nunterminated');
	assert.equal(memoryBody('  \n---\ncreated: x\n---\n\nbody line 1\nbody line 2\n'), 'body line 1\nbody line 2');
});

test('pin and exclude controls follow the flags and only exist for text memories', () => {
	assert.equal(canFlag('about/tea.md'), true);
	assert.equal(canFlag('photos/sky.png'), false);
	const off = flagControls({ pinned: false, exclude_from_proactive: false });
	assert.deepEqual(off.map((c) => [c.flag, c.active, c.label]), [
		['pinned', false, 'Pin'],
		['exclude_from_proactive', false, 'Exclude from proactive use'],
	]);
	const on = flagControls({ pinned: true, exclude_from_proactive: true });
	assert.deepEqual(on.map((c) => [c.active, c.label]), [[true, 'Unpin'], [true, 'Allow proactive use']]);
	assert.deepEqual(on.map((c) => c.next), [{ pinned: false }, { exclude_from_proactive: false }]);
	assert.deepEqual(off.map((c) => c.next), [{ pinned: true }, { exclude_from_proactive: true }]);
	assert.deepEqual(flagBadges({ pinned: true, exclude_from_proactive: false }), ['Pinned']);
	assert.deepEqual(flagBadges({ pinned: true, exclude_from_proactive: true }), ['Pinned', 'Not used proactively']);
	assert.deepEqual(flagBadges(undefined), []);
});

test('a memory cited once per vector chunk is shown once, as its best-ranked chunk', () => {
	// A memory longer than 600 bytes is indexed as several chunks; every
	// chunk that ranks is its own hit with the same path and reason.
	const first = { ...tea, path: 'about/runs.md', source: 'about/runs.md', excerpt: 'runs at dawn, 5 km', confidence: 'high' };
	const second = { ...first, excerpt: 'ran a half marathon in April', confidence: 'medium' };
	assert.deepEqual(uniqueMemories([first, second, sky]), [first, sky]);
	assert.deepEqual(uniqueMemories([]), []);

	const receipts = [
		{ message_id: 'msg_1', chat_id: 'default', memories: [first, second] },
		{ message_id: 'msg_2', chat_id: 'default', memories: [second, tea, first] },
	];
	const byMessage = receiptsByMessage(receipts, 'default');
	assert.deepEqual(byMessage.get('msg_1'), [first]);
	assert.equal(receiptSummary(byMessage.get('msg_1')), '1 memory shaped this reply');
	assert.deepEqual(byMessage.get('msg_2').map((m) => m.path), ['about/runs.md', 'about/tea.md']);
	assert.equal(byMessage.get('msg_2')[0].excerpt, 'ran a half marathon in April');

	// The library lists one recall per reply, so a chunked memory cannot key
	// the same conversation twice.
	const recalls = recallsOf(receipts, 'about/runs.md');
	assert.deepEqual(recalls.map((r) => r.message_id), ['msg_1', 'msg_2']);
	assert.equal(recalls[0].memory.excerpt, 'runs at dawn, 5 km');
	const keys = recalls.map((r) => `${r.chat_id}/${r.message_id}`);
	assert.equal(new Set(keys).size, keys.length);
});

test('a conflict that is still pending from an earlier correction is named as such, and the draft is kept', () => {
	const parked = {
		conflict_id: 'corr_2_b',
		path: 'about/tea.md',
		current: { id: 'corr_1_a', statement: 'likes oolong', corrected_at: '2026-09-20T09:00:00Z' },
		proposed: { id: 'corr_2_b', statement: 'likes matcha', corrected_at: '2026-09-20T10:00:00Z' },
	};
	// The server answers every further correction of a memory with the pair
	// it already parked, so the statement just typed is not in the prompt.
	assert.equal(conflictIsPending(parked, 'likes matcha'), false);
	assert.equal(conflictIsPending(parked, '  likes matcha \n'), false);
	assert.equal(conflictIsPending(parked, 'prefers herbal tea in the evening'), true);

	const fresh = conflictPrompt(parked, 'likes matcha');
	assert.equal(fresh.pending, false);
	assert.equal(fresh.question, 'Two of your corrections to tea disagree. Which one should stay?');
	assert.deepEqual(fresh.options.map((o) => o.title), ['Keep the current statement', 'Use the new statement']);
	assert.equal(fresh.note, 'Nothing is merged: the memory keeps the current statement until you choose.');
	assert.deepEqual(conflictPrompt(parked), fresh);

	const pending = conflictPrompt(parked, 'prefers herbal tea in the evening');
	assert.equal(pending.pending, true);
	assert.equal(pending.conflictId, 'corr_2_b');
	assert.equal(pending.question, 'An earlier correction of tea is still waiting for your decision. Settle it first, then save your new statement.');
	assert.equal(pending.note, 'Nothing is merged, and your new statement below is not applied yet: it stays in the editor until you save it again.');
	assert.deepEqual(pending.options.map((o) => [o.keep, o.title, o.statement]), [
		['current', 'Keep the current statement', 'likes oolong'],
		['proposed', 'Use the earlier correction', 'likes matcha'],
	]);
});

test('a media correction starts from the bound text, never the listing summary or the receipt excerpt', () => {
	assert.equal(boundTextPath('about/tea.md'), 'about/tea.md');
	assert.equal(boundTextPath('photos/sky.png'), 'photos/sky.png.md');
	assert.equal(boundTextPath('docs/brief.pdf'), 'docs/brief.pdf.md');

	const sidecar = 'NOLUNE_MEDIA_TEXT {"version":1,"sha256":"ab12"}\nSky over Lisbon at dusk.\nTaken from the balcony.\n';
	assert.equal(mediaBoundText(sidecar), 'Sky over Lisbon at dusk.\nTaken from the balcony.');
	assert.equal(mediaBoundText('NOLUNE_MEDIA_TEXT {"version":1}\n'), '');
	assert.equal(mediaBoundText('plain description'), 'plain description');
	assert.equal(mediaBoundText(''), '');

	for (const file of ['MemoryControls.svelte', 'MemoryReceiptPanel.svelte', 'MemoryLibraryView.svelte']) {
		const source = readFileSync(fileURLToPath(new URL(`../src/lib/components/memory/${file}`, import.meta.url)), 'utf8');
		assert.ok(!source.includes('excerpt={'), `${file} must not prefill a correction from an excerpt`);
		assert.ok(!/draft\s*=\s*(excerpt|summary)/.test(source), `${file} must not seed the editor from a summary or excerpt`);
	}
});

test('receipt and correction requests stay inside the current companion scope', () => {
	const source = readFileSync(fileURLToPath(new URL('../src/lib/api/client.ts', import.meta.url)), 'utf8');
	const fns = ['fetchMemoryReceipts', 'fetchMemoryReceipt', 'correctMemoryFile', 'setMemoryFlags', 'fetchMemoryCorrections', 'resolveMemoryCorrection'];
	for (const name of fns) {
		const start = source.indexOf(`export async function ${name}(`) >= 0
			? source.indexOf(`export async function ${name}(`)
			: source.indexOf(`export function ${name}(`);
		assert.ok(start >= 0, `${name} is declared in client.ts`);
		const body = source.slice(start, source.indexOf('\n}\n', start));
		const urls = body.match(/`\/api[^`]*`/g) ?? [];
		assert.ok(urls.length > 0, `${name} builds an API URL`);
		for (const url of urls) {
			assert.ok(url.startsWith('`/api/instances/${encodeURIComponent(slug)}/'), `${name} scopes ${url} to the companion`);
		}
	}
	for (const token of ['score.toFixed', 'content_preview']) {
		assert.ok(!source.includes(token), `client.ts must not expose ${token}`);
	}
});
