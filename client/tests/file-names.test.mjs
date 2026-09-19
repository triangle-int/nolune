import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { linkFileName, filenameFromContentDisposition } from '../src/lib/api/file-names.js';

const href = 'http://localhost:26559/resources/model-provider/files/companion/upload_1789842329655.md?cap=v1.eyJ0b2tlbiI6IngifQ';

test('a named link keeps its label as the file name', () => {
	assert.equal(linkFileName('sample.md', href), 'sample.md');
	assert.equal(linkFileName('  Quarterly report.pdf ', href), 'Quarterly report.pdf');
});

test('an autolinked URL falls back to the last path segment without the query', () => {
	assert.equal(linkFileName(href, href), 'upload_1789842329655.md');
	assert.equal(linkFileName('', href), 'upload_1789842329655.md');
	assert.equal(linkFileName('http://localhost:26559/resources/browser/files/companion/upload_1.md?cap=x#frag', href), 'upload_1789842329655.md');
	assert.equal(linkFileName('https://example.com/a%20b.txt', 'https://example.com/a%20b.txt'), 'a b.txt');
});

test('unusable links produce the generic name', () => {
	assert.equal(linkFileName('', 'not a url'), 'file');
	assert.equal(linkFileName('', 'https://example.com/'), 'file');
});

test('content-disposition filenames are honoured over the viewer name', () => {
	assert.equal(filenameFromContentDisposition('attachment; filename="sample.md"'), 'sample.md');
	assert.equal(filenameFromContentDisposition('inline; filename=photo.png'), 'photo.png');
	assert.equal(filenameFromContentDisposition("attachment; filename=\"fallback.txt\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"), 'résumé.pdf');
	assert.equal(filenameFromContentDisposition('attachment'), null);
	assert.equal(filenameFromContentDisposition(null), null);
	assert.equal(filenameFromContentDisposition('attachment; filename="../evil.sh"'), 'evil.sh');
});

test('chat links and the viewer download use the shared helpers', () => {
	const bubble = readFileSync(new URL('../src/lib/components/chat/MessageBubble.svelte', import.meta.url), 'utf8');
	assert.match(bubble, /linkFileName\(/);
	const viewer = readFileSync(new URL('../src/lib/components/FileViewer.svelte', import.meta.url), 'utf8');
	assert.match(viewer, /filenameFromContentDisposition\(/);
});
