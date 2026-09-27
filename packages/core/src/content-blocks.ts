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
