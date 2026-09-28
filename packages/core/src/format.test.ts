import { describe, expect, it } from 'vitest';
import { toAnthropicBlocks, toAnthropicMessages } from './anthropic.ts';
import { readMessage, type Block, type StoredRow } from './format.ts';
import { toResponsesInput } from './openai-chat.ts';

/** A row as stored: in nolune's format, or as rows were stored before it (Anthropic's shape). */
function row(content: unknown[], options: Partial<Omit<StoredRow, 'content'>> = {}): StoredRow {
	return {
		role: 'user',
		format: null,
		provider: 'anthropic',
		model: null,
		...options,
		content: JSON.stringify(content)
	};
}

const photo = { type: 'image', source: { type: 'file', file_id: 'file_photo' } };
const dot = {
	type: 'image',
	source: { type: 'base64', media_type: 'image/png', data: 'iVBORw0KGgo=' }
};
const menu = {
	type: 'document',
	source: { type: 'file', file_id: 'file_menu' },
	title: 'menu.pdf'
};

const searchResult = { type: 'search_result', source: 'x', title: 'y', content: [] };

describe("rows from before nolune's own format", () => {
	// Every shape nolune wrote, and a few it didn't, as they are in chats people have.
	const stored = [
		{ type: 'text', text: '[Anna attached photo.jpg, saved at /photo.jpg]' },
		photo,
		dot,
		menu,
		{ type: 'text', text: 'Anna: What is this?' },
		{ type: 'tool_result', tool_use_id: 't1', content: 'a.txt' },
		{ type: 'tool_result', tool_use_id: 't2', content: 'x', is_error: false },
		{ type: 'tool_result', tool_use_id: 't3', content: 'Stopped.', is_error: true },
		{
			type: 'tool_result',
			tool_use_id: 't4',
			content: [{ type: 'text', text: 'Image: /dot.png' }, dot, photo]
		},
		{ type: 'search_result', source: 'https://example.com', title: 'Kept', content: [] },
		{ type: 'tool_result', tool_use_id: 't5', content: [searchResult] }
	];

	it('go to Claude byte for byte', () => {
		const message = readMessage(row(stored));
		expect(JSON.stringify(toAnthropicBlocks(message.blocks))).toBe(JSON.stringify(stored));
		expect(toAnthropicMessages([message])).toEqual([{ role: 'user', content: stored }]);
	});

	it('are read as what they say', () => {
		expect(readMessage(row(stored, { provider: 'openai' })).blocks).toEqual([
			{ type: 'text', text: '[Anna attached photo.jpg, saved at /photo.jpg]' },
			{ type: 'image', source: { type: 'uploaded', provider: 'openai', fileId: 'file_photo' } },
			{ type: 'image', source: { type: 'inline', mime: 'image/png', data: 'iVBORw0KGgo=' } },
			{
				type: 'pdf',
				source: { type: 'uploaded', provider: 'openai', fileId: 'file_menu' },
				name: 'menu.pdf'
			},
			{ type: 'text', text: 'Anna: What is this?' },
			{ type: 'tool_result', callId: 't1', content: 'a.txt', isError: false },
			{ type: 'tool_result', callId: 't2', content: 'x', isError: false },
			{ type: 'tool_result', callId: 't3', content: 'Stopped.', isError: true },
			{
				type: 'tool_result',
				callId: 't4',
				content: [
					{ type: 'text', text: 'Image: /dot.png' },
					{ type: 'image', source: { type: 'inline', mime: 'image/png', data: 'iVBORw0KGgo=' } },
					{ type: 'image', source: { type: 'uploaded', provider: 'openai', fileId: 'file_photo' } }
				],
				isError: false
			},
			{ type: 'other', anthropic: stored[9] },
			{
				type: 'tool_result',
				callId: 't5',
				content: [{ type: 'other', anthropic: searchResult }],
				isError: false
			}
		]);
	});

	it("keep all they had when Claude can't open a file in them", () => {
		const result = {
			type: 'tool_result',
			tool_use_id: 't4',
			content: [dot, photo],
			is_error: false
		};
		const blocks = readMessage(row([photo, result], { provider: 'openai' })).blocks;
		const note = expect.objectContaining({ type: 'text' });
		expect(toAnthropicBlocks(blocks)).toEqual([
			note,
			{ type: 'tool_result', tool_use_id: 't4', content: [dot, note], is_error: false }
		]);
	});
});

describe('results Claude Code wrote itself', () => {
	it('keep what nolune has no block for, for Claude', () => {
		const unknown = { type: 'tool_reference', tool_name: 'run_command' };
		const result = {
			type: 'tool_result',
			tool_use_id: 't1',
			content: [{ type: 'text', text: 'Denied.' }, unknown]
		};
		// Stored in nolune's format, then read back.
		const saved = JSON.stringify(readMessage(row([result])).blocks);
		const blocks = readMessage(row(JSON.parse(saved), { format: 'nolune' })).blocks;
		expect(toAnthropicBlocks(blocks)).toEqual([result]);
		expect(toResponsesInput([{ role: 'user', blocks }], 'gpt-6-astra')).toEqual([
			{
				type: 'function_call_output',
				call_id: 't1',
				output: [{ type: 'input_text', text: 'Denied.' }]
			}
		]);
	});
});

describe("nolune's own format", () => {
	const blocks: Block[] = [
		{ type: 'text', text: '[Anna attached photo.jpg, saved at /photo.jpg]' },
		{ type: 'image', source: { type: 'uploaded', provider: 'anthropic', fileId: 'file_photo' } },
		{ type: 'image', source: { type: 'inline', mime: 'image/png', data: 'iVBORw0KGgo=' } },
		{
			type: 'pdf',
			source: { type: 'uploaded', provider: 'anthropic', fileId: 'file_menu' },
			name: 'menu.pdf'
		},
		{ type: 'text', text: 'Anna: What is this?' },
		{ type: 'tool_result', callId: 't1', content: 'a.txt', isError: false },
		{ type: 'tool_result', callId: 't3', content: 'Stopped.', isError: true },
		{
			type: 'tool_result',
			callId: 't4',
			content: [
				{ type: 'text', text: 'Image: /dot.png' },
				{ type: 'image', source: { type: 'inline', mime: 'image/png', data: 'iVBORw0KGgo=' } }
			],
			isError: false
		}
	];

	it('gives Claude what nolune sent it before it had its own format', () => {
		const message = readMessage(row(blocks, { format: 'nolune' }));
		expect(JSON.stringify(toAnthropicBlocks(message.blocks))).toBe(
			JSON.stringify([
				{ type: 'text', text: '[Anna attached photo.jpg, saved at /photo.jpg]' },
				photo,
				dot,
				menu,
				{ type: 'text', text: 'Anna: What is this?' },
				{ type: 'tool_result', tool_use_id: 't1', content: 'a.txt' },
				{ type: 'tool_result', tool_use_id: 't3', content: 'Stopped.', is_error: true },
				{
					type: 'tool_result',
					tool_use_id: 't4',
					content: [{ type: 'text', text: 'Image: /dot.png' }, dot]
				}
			])
		);
	});

	it('gives OpenAI the same as the rows from before it', () => {
		const nolune = readMessage(row(blocks, { format: 'nolune', provider: 'openai' }));
		const before = readMessage(
			row(JSON.parse(JSON.stringify(toAnthropicBlocks(nolune.blocks))), { provider: 'anthropic' })
		);
		// Files OpenAI holds, in both.
		const own = (message: typeof nolune): typeof nolune => ({
			...message,
			blocks: message.blocks.map((b) =>
				(b.type === 'image' || b.type === 'pdf') && b.source.type === 'uploaded'
					? { ...b, source: { ...b.source, provider: 'openai' } }
					: b
			)
		});
		expect(toResponsesInput([own(nolune)], 'gpt-6-astra')).toEqual(
			toResponsesInput([own(before)], 'gpt-6-astra')
		);
		expect(toResponsesInput([own(nolune)], 'gpt-6-astra')).toEqual([
			{
				role: 'user',
				content: [
					{ type: 'input_text', text: '[Anna attached photo.jpg, saved at /photo.jpg]' },
					{ type: 'input_image', file_id: 'file_photo', detail: 'auto' },
					{
						type: 'input_image',
						image_url: 'data:image/png;base64,iVBORw0KGgo=',
						detail: 'auto'
					},
					{ type: 'input_file', file_id: 'file_menu' },
					{ type: 'input_text', text: 'Anna: What is this?' }
				]
			},
			{ type: 'function_call_output', call_id: 't1', output: 'a.txt' },
			{ type: 'function_call_output', call_id: 't3', output: 'Stopped.' },
			{
				type: 'function_call_output',
				call_id: 't4',
				output: [
					{ type: 'input_text', text: 'Image: /dot.png' },
					{
						type: 'input_image',
						image_url: 'data:image/png;base64,iVBORw0KGgo=',
						detail: 'auto'
					}
				]
			}
		]);
	});
});

describe('replies', () => {
	const claude = [
		{ type: 'thinking', thinking: 'Listing.', signature: 'sig' },
		{ type: 'text', text: 'Looking.' },
		{ type: 'tool_use', id: 'toolu_1', name: 'run_command', input: { command: 'ls' } }
	];

	it('keep what their provider returned, and read as nolune blocks', () => {
		const reply = readMessage(
			row(claude, { role: 'assistant', provider: 'anthropic', model: 'claude-sonnet-5' })
		);
		expect(reply).toEqual({
			role: 'assistant',
			blocks: [
				{ type: 'reasoning', text: 'Listing.' },
				{ type: 'text', text: 'Looking.' },
				{ type: 'tool_call', id: 'toolu_1', name: 'run_command', input: { command: 'ls' } }
			],
			native: { provider: 'anthropic', model: 'claude-sonnet-5', content: claude }
		});
		expect(toAnthropicMessages([reply])).toEqual([{ role: 'assistant', content: claude }]);
	});

	it('that nolune wrote itself go to any model as their text', () => {
		const reply = readMessage(
			row([{ type: 'text', text: 'Rain at 4pm.' }], { role: 'assistant', format: 'nolune' })
		);
		expect(reply.native).toBeUndefined();
		expect(toAnthropicMessages([reply])).toEqual([
			{ role: 'assistant', content: [{ type: 'text', text: 'Rain at 4pm.' }] }
		]);
		expect(toResponsesInput([reply], 'gpt-6-astra')).toEqual([
			{ role: 'assistant', content: 'Rain at 4pm.' }
		]);
	});
});
