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

/** Markdown to one line of plain text, for tooltips. */
function plain(markdown: string): string {
	return markdown
		.replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
		.replace(/(\*\*|__)(.+?)\1/g, '$2')
		.replace(/(?<!\w)([*_])(.+?)\1(?!\w)/g, '$2')
		.replace(/~~(.+?)~~/g, '$1')
		.replace(/`([^`]+)`/g, '$1')
		.replace(/\s+/g, ' ')
		.trim();
}

/**
 * The separate things a memory file says: list items, paragraphs, table rows and code blocks.
 * Headings only group them. The agent is asked to write one fact per bullet, so this is usually
 * one entry per fact.
 */
export function memoryFacts(text: string): string[] {
	const facts: string[] = [];
	let current: string[] = [];
	let fence = false;
	let lastWasRow = false;
	const flush = () => {
		if (current.length) facts.push(current.join(' '));
		current = [];
	};
	for (const raw of text.split('\n')) {
		const line = raw.trim();
		if (/^(```|~~~)/.test(line)) {
			fence = !fence;
			if (!fence) flush();
			continue;
		}
		if (fence) {
			if (line) current.push(line);
			continue;
		}
		const isRow = line.startsWith('|');
		if (isRow && /^\|?[\s:|-]+$/.test(line)) {
			// A table's separator: the row above it was the header, not a fact.
			if (lastWasRow) facts.pop();
			continue;
		}
		lastWasRow = isRow;
		if (!line || /^#{1,6}(\s|$)/.test(line) || /^([-*_])(\s*\1){2,}$/.test(line)) {
			flush();
			continue;
		}
		if (isRow) {
			flush();
			facts.push(
				line
					.replace(/^\||\|$/g, '')
					.split('|')
					.map((cell) => cell.trim())
					.filter(Boolean)
					.join(' · ')
			);
			continue;
		}
		const item = line.match(/^(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?(.*)$/);
		if (item) {
			flush();
			current.push(item[1]);
		} else {
			current.push(line.replace(/^>\s?/, ''));
		}
	}
	flush();
	return facts.map(plain).filter(Boolean);
}
