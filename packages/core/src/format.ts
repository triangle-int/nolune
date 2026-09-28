import type { Provider } from './models.ts';

/*
 * nolune's own format for what a conversation holds, whichever provider its model runs on. Each
 * provider's module turns it into its own request (`toAnthropicMessages` in anthropic.ts, which
 * the Claude plan uses too, `toResponsesInput` in openai-chat.ts and `toChatMessages` in
 * openrouter.ts), leaving out what that provider can't take. Another provider is one more such
 * function.
 *
 * What nolune writes itself (people's messages, command results, notices, its own replies) is stored
 * in this format (`message.format` is 'nolune'). A reply from a model is stored exactly as its
 * provider returned it, and read into this format with that original kept (`native`): only the
 * model that wrote it can read its reasoning back, and only unchanged.
 *
 * Rows from before this format are in Anthropic's shape: nolune's own blocks then were Anthropic's
 * content blocks. They are read the same way, and each block keeps what was stored (`storedAs`),
 * which Claude gets back byte for byte, so conversations from then keep their cache and thinking.
 */

/** Providers with a Files API, where pictures and PDFs can be kept. */
export type FileProvider = 'anthropic' | 'openai' | 'openrouter';

/** Where a picture's or PDF's bytes are. */
export type Source =
	/**
	 * Kept by nolune, in its media store (`media/<sha256>`). Each provider gets its own copy when a
	 * request is made (resolveFiles in provider-files.ts): an upload to its Files API, or inline.
	 * `bytes` counts against the conversation's inline limit where they go inline.
	 */
	| { type: 'media'; sha256: string; mime: string; bytes: number }
	/** In the message itself, as base64: every provider takes it. */
	| { type: 'inline'; mime: string; data: string }
	/**
	 * A copy in a provider's Files API, which only that provider can open. Null: a row that didn't
	 * record whose it is (from tests), which goes to any provider as it is.
	 */
	| { type: 'uploaded'; provider: FileProvider | null; fileId: string };

export interface TextBlock {
	type: 'text';
	text: string;
}

export interface ImageBlock {
	type: 'image';
	source: Source;
}

export interface PdfBlock {
	type: 'pdf';
	source: Source;
	/** The file's name, which the model sees. */
	name: string;
}

/** A reply's reasoning, as far as people may read it: a summary, or nothing. */
export interface ReasoningBlock {
	type: 'reasoning';
	text: string;
}

export interface ToolCallBlock {
	type: 'tool_call';
	id: string;
	name: string;
	input: unknown;
}

/** What a tool call printed, or that and the pictures it opened (`nolune view`). */
export interface ToolResultBlock {
	type: 'tool_result';
	callId: string;
	content: string | ResultBlock[];
	isError: boolean;
}

/**
 * A block nolune doesn't know, in Anthropic's shape: from a row from before this format, or a result
 * Claude Code wrote itself. It goes only to Claude, as it is.
 */
export interface OtherBlock {
	type: 'other';
	anthropic: unknown;
}

export type ResultBlock = TextBlock | ImageBlock | PdfBlock | OtherBlock;
export type Block =
	TextBlock | ImageBlock | PdfBlock | ReasoningBlock | ToolCallBlock | ToolResultBlock | OtherBlock;

/** A reply as its provider returned it. */
export interface Native {
	/** Null: rows that didn't record who wrote them (from tests), sent to any provider as they are. */
	provider: Provider | null;
	model: string | null;
	content: unknown[];
}

export interface Message {
	role: 'user' | 'assistant';
	/** For a reply with `native`: what it says, read from that, without what only its model reads. */
	blocks: Block[];
	/** A reply from a model, as it came. */
	native?: Native;
	/**
	 * A reply from before the system prompt was built again. Claude's thinking is bound to the
	 * prompt it was made under, so it's left out of such replies (anthropic.ts).
	 */
	beforePromptChange?: boolean;
}

/** A row as stored in `message`. */
export interface StoredRow {
	role: 'user' | 'assistant';
	content: string;
	/** 'nolune': in this format. Null: a reply as its provider returned it, or a row from before. */
	format: 'nolune' | null;
	/** The provider that wrote a reply, or whose Files API a row's pictures and PDFs went to. */
	provider: Provider | null;
	model: string | null;
}

// --- reading stored rows ---

type Stored = { type?: unknown } & Record<string, unknown>;

/** Blocks of rows from before this format, and what was stored for each. */
const stored = new WeakMap<Block, unknown>();

/**
 * What a block of a row from before this format was stored as (Anthropic's shape), which Claude
 * gets back unchanged. Undefined for any other block, and for a copy of one.
 */
export function storedAs(block: Block): unknown {
	return stored.get(block);
}

export function readMessage(row: StoredRow): Message {
	const content = JSON.parse(row.content) as unknown;
	if (row.format === 'nolune') return { role: row.role, blocks: content as Block[] };
	if (row.role === 'assistant') {
		const native = Array.isArray(content) ? content : [];
		return {
			role: 'assistant',
			blocks: replyBlocks(native),
			native: { provider: row.provider, model: row.model, content: native }
		};
	}
	const uploadedTo =
		row.provider === 'anthropic' || row.provider === 'openai' || row.provider === 'openrouter'
			? row.provider
			: null;
	return {
		role: 'user',
		blocks: (Array.isArray(content) ? (content as Stored[]) : []).map((b) =>
			fromAnthropic(b, uploadedTo)
		)
	};
}

/**
 * One of Anthropic's content blocks in a message (as nolune's own rows were stored before this
 * format, and as Claude Code hands over results), remembering what it was.
 */
export function fromAnthropic(block: unknown, uploadedTo: FileProvider | null): Block {
	const read = readAnthropic(block as Stored, uploadedTo);
	stored.set(read, block);
	return read;
}

/** A block inside a tool result, where only text, pictures and PDFs belong. */
function resultBlock(block: Stored, uploadedTo: FileProvider | null): ResultBlock {
	const read = fromAnthropic(block, uploadedTo);
	if (read.type === 'text' || read.type === 'image' || read.type === 'pdf') return read;
	const other: OtherBlock = { type: 'other', anthropic: block };
	stored.set(other, block);
	return other;
}

function readAnthropic(block: Stored, uploadedTo: FileProvider | null): Block {
	switch (block?.type) {
		case 'text':
			return { type: 'text', text: str(block.text) };
		case 'image': {
			const source = readSource(block.source, uploadedTo);
			return source ? { type: 'image', source } : { type: 'other', anthropic: block };
		}
		case 'document': {
			const source = readSource(block.source, uploadedTo);
			return source
				? { type: 'pdf', source, name: str(block.title) }
				: { type: 'other', anthropic: block };
		}
		case 'tool_result':
			return {
				type: 'tool_result',
				callId: str(block.tool_use_id),
				content: Array.isArray(block.content)
					? (block.content as Stored[]).map((b) => resultBlock(b, uploadedTo))
					: str(block.content),
				isError: block.is_error === true
			};
		default:
			return { type: 'other', anthropic: block };
	}
}

function readSource(source: unknown, uploadedTo: FileProvider | null): Source | null {
	const s = (source ?? {}) as Record<string, unknown>;
	if (s.type === 'base64') return { type: 'inline', mime: str(s.media_type), data: str(s.data) };
	if (s.type === 'file') return { type: 'uploaded', provider: uploadedTo, fileId: str(s.file_id) };
	return null;
}

/**
 * OpenAI and OpenRouter send a call's input as a JSON string. One that doesn't parse is passed on
 * as it is.
 */
export function parseToolArguments(args: unknown): unknown {
	if (typeof args !== 'string') return args ?? {};
	try {
		return JSON.parse(args) as unknown;
	} catch {
		return args;
	}
}

function str(value: unknown): string {
	return typeof value === 'string' ? value : '';
}

export type ReplyBlock = TextBlock | ReasoningBlock | ToolCallBlock;

/**
 * A reply in any provider's shape as nolune's blocks, in order: Anthropic's content blocks (`text`,
 * `thinking`, `tool_use`), OpenAI's output items (`message`, `reasoning`, `function_call`), or
 * OpenRouter's pieces: reasoning details (`reasoning.text`, `reasoning.summary`,
 * `reasoning.encrypted`) and tool calls (`function`) around a `text` block. They use different
 * type names, so no provider needs to be known. Anything else is skipped.
 */
export function replyBlocks(content: unknown): ReplyBlock[] {
	if (!Array.isArray(content)) return [];
	const blocks: ReplyBlock[] = [];
	for (const block of content as Stored[]) {
		switch (block?.type) {
			// Anthropic, and nolune's own replies from before this format
			case 'text':
				blocks.push({ type: 'text', text: str(block.text) });
				break;
			case 'thinking':
				blocks.push({ type: 'reasoning', text: str(block.thinking) });
				break;
			case 'tool_use':
				blocks.push({
					type: 'tool_call',
					id: str(block.id),
					name: str(block.name),
					input: block.input
				});
				break;
			// OpenAI's Responses API
			case 'message':
				for (const part of (Array.isArray(block.content) ? block.content : []) as Stored[]) {
					if (part?.type === 'output_text') blocks.push({ type: 'text', text: str(part.text) });
					else if (part?.type === 'refusal') {
						blocks.push({ type: 'text', text: str(part.refusal) });
					}
				}
				break;
			case 'reasoning': {
				const summary = (Array.isArray(block.summary) ? block.summary : []) as Stored[];
				const text = summary.map((s) => str(s?.text)).join('\n\n');
				blocks.push({ type: 'reasoning', text });
				break;
			}
			case 'function_call':
				blocks.push({
					type: 'tool_call',
					id: str(block.call_id),
					name: str(block.name),
					input: parseToolArguments(block.arguments)
				});
				break;
			// OpenRouter's Chat Completions. Encrypted reasoning has nothing to show.
			case 'reasoning.text':
				blocks.push({ type: 'reasoning', text: str(block.text) });
				break;
			case 'reasoning.summary':
				blocks.push({ type: 'reasoning', text: str(block.summary) });
				break;
			case 'function': {
				const fn = (block.function ?? {}) as Stored;
				blocks.push({
					type: 'tool_call',
					id: str(block.id),
					name: str(fn.name),
					input: parseToolArguments(fn.arguments)
				});
				break;
			}
		}
	}
	return blocks;
}

/** The tool calls in a reply as its provider returned it, in order. */
export function toolCalls(content: unknown): ToolCallBlock[] {
	return replyBlocks(content).filter((b) => b.type === 'tool_call');
}

// --- helpers for every provider ---

/**
 * A reply's text and tool calls, which any model can read: how a reply goes to a model other than
 * the one that wrote it. Its reasoning is left out, and so are empty texts (the APIs refuse them)
 * and calls without an id. A call whose input isn't an object goes without it; its result says it
 * was invalid.
 */
export function portableReply(blocks: Block[]): (TextBlock | ToolCallBlock)[] {
	return blocks.flatMap((b): (TextBlock | ToolCallBlock)[] => {
		if (b.type === 'text') return b.text.trim() ? [b] : [];
		if (b.type !== 'tool_call' || !b.id) return [];
		const input = b.input && typeof b.input === 'object' && !Array.isArray(b.input) ? b.input : {};
		return [input === b.input ? b : { ...b, input }];
	});
}

/** Whether a picture or PDF is kept by another provider than `provider`, which can't open it. */
export function heldElsewhere(block: ImageBlock | PdfBlock, provider: Provider): boolean {
	return (
		block.source.type === 'uploaded' &&
		block.source.provider !== null &&
		block.source.provider !== provider
	);
}

/** What a model reads instead of a picture or PDF another provider holds. */
export function heldElsewhereNote(block: ImageBlock | PdfBlock): TextBlock {
	const what = block.type === 'image' ? 'Picture' : 'PDF';
	return {
		type: 'text',
		text: `[${what} not shown: it went to the model this chat used before, and this model can't open that copy. The line before this says where its file is${block.type === 'image' ? '; `nolune view` shows it again' : ''}.]`
	};
}

/**
 * For encoders given a picture or PDF by reference: resolveFiles gives each provider its copy
 * before a request is built.
 */
export function unresolved(block: ImageBlock | PdfBlock): never {
	throw new Error(`A ${block.type} kept by reference reached a request without its copy.`);
}

/** A tool result's text, with a placeholder for each picture or PDF. */
export function resultText(content: ToolResultBlock['content']): string {
	if (typeof content === 'string') return content;
	return content.map((b) => (b.type === 'text' ? b.text : `[${placeholder(b)}]`)).join('\n');
}

/** What stands for a picture, PDF or unknown block in text: `image`, `document`... */
export function placeholder(block: ResultBlock): string {
	if (block.type === 'pdf') return 'document';
	if (block.type !== 'other') return block.type;
	return str((block.anthropic as Stored | null)?.type) || 'other';
}

/** The text of a message's blocks, and what its tool results printed. */
export function messageText(blocks: Block[]): string {
	return blocks
		.map((b) =>
			b.type === 'text' ? b.text : b.type === 'tool_result' ? resultText(b.content) : ''
		)
		.filter(Boolean)
		.join('\n')
		.trim();
}
