import test from 'node:test';
import assert from 'node:assert/strict';
import { formatBytes, importReply, importStatusText } from '../src/lib/settings/import-status.js';

test('the status line says Uploading until the upload is reported complete, even for an empty file', () => {
	assert.equal(importStatusText({ importing: false, sent: 0, total: 0, uploaded: false }), '');
	// Before the first progress event nothing is known about the body yet.
	assert.equal(importStatusText({ importing: true, sent: 0, total: 0, uploaded: false }), 'Uploading…');
	assert.equal(
		importStatusText({ importing: true, sent: 512, total: 2048, uploaded: false }),
		'Uploading… 512 B of 2 KB',
	);
	// The upload is done only when the request said so, not when the numbers happen to match.
	assert.equal(
		importStatusText({ importing: true, sent: 2048, total: 2048, uploaded: false }),
		'Uploading… 2 KB of 2 KB',
	);
	assert.match(importStatusText({ importing: true, sent: 2048, total: 2048, uploaded: true }), /^Restoring…/);
	assert.match(importStatusText({ importing: true, sent: 0, total: 0, uploaded: true }), /^Restoring…/);
});

test('a reply is read as the import answer only when it is one', () => {
	const ok = importReply(200, JSON.stringify({ ok: true, files: 3, bytes: 10, derived_index: 'pending', indexed_chunks: 0 }));
	assert.deepEqual(ok, { ok: true, outcome: { ok: true, files: 3, bytes: 10, derived_index: 'pending', indexed_chunks: 0 } });

	const busy = importReply(409, JSON.stringify({ error: 'companion_busy', message: '1 agent task is running' }));
	assert.deepEqual(busy, { ok: false, code: 'companion_busy', message: '1 agent task is running' });

	// An error body that is not JSON keeps its text; an empty one never yields an empty message.
	assert.deepEqual(importReply(502, 'bad gateway'), { ok: false, code: 'http_502', message: 'bad gateway' });
	assert.deepEqual(importReply(502, ''), { ok: false, code: 'http_502', message: 'import failed' });
	assert.deepEqual(importReply(500, JSON.stringify({ error: 'import_failed', message: '' })), {
		ok: false,
		code: 'import_failed',
		message: 'import failed',
	});

	// A 2xx without the import's own answer is not a restore.
	const page = importReply(200, '<html>ok</html>');
	assert.equal(page.ok, false);
	assert.equal(page.code, 'unexpected_reply');
	assert.match(page.message, /cannot tell whether/);
	assert.equal(importReply(200, '').ok, false);
	assert.equal(importReply(200, JSON.stringify({ files: 3 })).ok, false);
});

test('byte counts are short and human', () => {
	assert.equal(formatBytes(0), '0 B');
	assert.equal(formatBytes(1023), '1023 B');
	assert.equal(formatBytes(1024), '1 KB');
	assert.equal(formatBytes(1536 * 1024), '1.5 MB');
});
