import type { MemoryCall } from '@btw/core';

/** The command's first line, for the compact technical label. */
export function firstLine(command: string, max = 120): string {
	const line = command.trim().split('\n')[0];
	return line.length > max ? line.slice(0, max) + '…' : line;
}

export interface ToolInput {
	command: string | null;
	/** Plain-language description the model writes with each call. */
	summary: string | null;
	/** Lucide icon name the model picked for the call. */
	icon: string | null;
}

/** Reads one string field out of tool input JSON that may still be streaming in. */
function partialString(json: string, key: string): string | null {
	const match = json.match(new RegExp(`"${key}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)`));
	if (!match) return null;
	// The text may stop halfway through an escape like é; drop the unfinished tail.
	for (let cut = 0; cut <= 5; cut++) {
		try {
			return JSON.parse(`"${match[1].slice(0, match[1].length - cut)}"`) as string;
		} catch {
			// Try a shorter prefix.
		}
	}
	return match[1];
}

/** The fields of a `run_command` call whose input JSON is still streaming in. */
export function partialToolInput(json: string): ToolInput {
	let input: Record<string, unknown> | null = null;
	try {
		input = JSON.parse(json) as Record<string, unknown>;
	} catch {
		// Incomplete; read the fields one by one below.
	}
	const field = (key: string) => {
		const value = input ? input[key] : partialString(json, key);
		return typeof value === 'string' && value.trim() ? value.trim() : null;
	};
	// Only a finished icon name: "cloud" on the way to "cloud-sun" would flash the wrong icon.
	const icon = input ? field('icon') : (json.match(/"icon"\s*:\s*"([^"\\]+)"/)?.[1] ?? null);
	return { command: field('command'), summary: field('summary'), icon };
}

/** The fields of a memory tool call whose input JSON is still streaming in. */
export function partialMemoryCall(json: string): MemoryCall {
	let input: Record<string, unknown> | null = null;
	try {
		input = JSON.parse(json) as Record<string, unknown>;
	} catch {
		// Incomplete; read the fields one by one below.
	}
	const field = (key: string) => {
		const value = input ? input[key] : partialString(json, key);
		return typeof value === 'string' ? value : undefined;
	};
	/** Only whole values: a path cut off halfway would flash the wrong topic. */
	const whole = (key: string) => {
		if (input) return field(key);
		const match = json.match(new RegExp(`"${key}"\\s*:\\s*("(?:[^"\\\\]|\\\\.)*")`));
		return match ? (JSON.parse(match[1]) as string) : undefined;
	};
	const text = field('file_text') ?? field('new_str') ?? field('insert_text');
	const newPath = whole('new_path');
	const oldText = field('old_str');
	return {
		command: whole('command') ?? null,
		path: whole('path') ?? whole('old_path') ?? null,
		...(newPath !== undefined ? { newPath } : {}),
		...(text !== undefined ? { text } : {}),
		...(oldText !== undefined ? { oldText } : {})
	};
}
