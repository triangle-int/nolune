import type { MemoryCall } from '@btw/core';

const ROOT = '/memories';

/** A file's name as a topic: `people/anna-smith.md` is "Anna smith". */
export function memoryTopic(path: string): string {
	const name = path.replace(/\/+$/, '').split('/').pop() ?? '';
	const words = name
		.replace(/\.[^.]+$/, '')
		.replace(/[-_]+/g, ' ')
		.trim();
	return words ? words.charAt(0).toUpperCase() + words.slice(1) : 'Memory';
}

/** Where a file's card is on the Memory page. */
export function memoryAnchor(path: string): string {
	return `memory-${path.replace(/^\/memories\/?/, '').replace(/[^a-zA-Z0-9]+/g, '-')}`;
}

function topicOf(call: MemoryCall): string | null {
	const path = call.path?.replace(/\/+$/, '');
	return path && path !== ROOT ? `“${memoryTopic(path)}”` : null;
}

/** What a memory call does, in plain words, like the summaries the model writes for commands. */
export function memoryStepLabel(call: MemoryCall): string {
	const topic = topicOf(call);
	switch (call.command) {
		case 'view':
			if (!topic) return 'Checking memory';
			// Folders have no extension.
			return /\.[^/]+$/.test(call.path ?? '')
				? `Reading ${topic} from memory`
				: `Looking through ${topic} in memory`;
		case 'create':
			return topic ? `Saving ${topic} to memory` : 'Saving to memory';
		case 'str_replace':
		case 'insert':
			return topic ? `Updating ${topic} in memory` : 'Updating memory';
		case 'delete':
			return topic ? `Forgetting ${topic}` : 'Forgetting something';
		case 'rename':
			return topic && call.newPath
				? `Renaming ${topic} to “${memoryTopic(call.newPath)}” in memory`
				: 'Tidying up memory';
		default:
			return 'Using memory';
	}
}

/** The call as it was made, for the technical view. */
export function memoryStepCommand(call: MemoryCall): string {
	const parts = ['memory', call.command ?? '…', call.path ?? ''];
	if (call.newPath) parts.push('→', call.newPath);
	return parts.join(' ').trim();
}

/** A Lucide icon name for the step. */
export function memoryStepIcon(call: MemoryCall): string {
	switch (call.command) {
		case 'view':
			return topicOf(call) && /\.[^/]+$/.test(call.path ?? '') ? 'book-open' : 'brain';
		case 'create':
			return 'bookmark-plus';
		case 'str_replace':
		case 'insert':
		case 'rename':
			return 'pencil-line';
		case 'delete':
			return 'eraser';
		default:
			return 'brain';
	}
}
