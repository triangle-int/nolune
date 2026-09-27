import type Anthropic from '@anthropic-ai/sdk';

/*
 * Reading stored message content, whichever provider wrote it. What btw writes itself (people's
 * messages, command results, notices) uses Anthropic's content blocks for every provider. A reply
 * is stored exactly as its provider returned it: Anthropic's content blocks (`text`, `thinking`,
 * `tool_use`), or OpenAI's output items (`message`, `reasoning`, `function_call`). The two use
 * different type names, so a row can be read without knowing its conversation's provider.
 */

/** A piece of a reply, as the chat shows it and the runner acts on it. */
export type ReplyBlock =
	| { type: 'text'; text: string }
	| { type: 'thinking'; text: string }
	| { type: 'tool_call'; id: string; name: string; input: unknown };

type Stored = { type?: unknown } & Record<string, unknown>;

/** OpenAI sends a call's input as a JSON string. One that doesn't parse is passed on as it is. */
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

/** The blocks of a reply in either provider's format, in order. Anything else is skipped. */
export function replyBlocks(content: unknown): ReplyBlock[] {
	if (!Array.isArray(content)) return [];
	const blocks: ReplyBlock[] = [];
	for (const block of content as Stored[]) {
		switch (block?.type) {
			// Anthropic, and btw's own replies (a notification continued in a chat)
			case 'text':
				blocks.push({ type: 'text', text: str(block.text) });
				break;
			case 'thinking':
				blocks.push({ type: 'thinking', text: str(block.thinking) });
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
				blocks.push({ type: 'thinking', text });
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
		}
	}
	return blocks;
}

/** The tool calls in a reply, in order. */
export function toolCalls(content: unknown): Extract<ReplyBlock, { type: 'tool_call' }>[] {
	return replyBlocks(content).filter((b) => b.type === 'tool_call');
}

/**
 * A reply in either provider's format as what any model can read: its text and tool calls, as
 * Anthropic's blocks, without reasoning, which only the model (or provider) that wrote it can
 * read back. This is how a reply goes to another model after the conversation switched. The same
 * content always gives the same blocks, so the request prefix stays byte-identical.
 */
export function portableReply(
	content: unknown
): (Anthropic.TextBlockParam | Anthropic.ToolUseBlockParam)[] {
	return replyBlocks(content).flatMap(
		(b): (Anthropic.TextBlockParam | Anthropic.ToolUseBlockParam)[] => {
			// The API refuses empty text blocks.
			if (b.type === 'text') return b.text.trim() ? [{ type: 'text', text: b.text }] : [];
			if (b.type !== 'tool_call' || !b.id) return [];
			// A call whose input didn't parse goes without it; its result says it was invalid.
			const input =
				b.input && typeof b.input === 'object' && !Array.isArray(b.input) ? b.input : {};
			return [{ type: 'tool_use', id: b.id, name: b.name, input }];
		}
	);
}
