/** A note's name as a topic: `people/anna-smith.md` is "Anna smith". */
export function memoryTopic(path: string): string {
	const name = path.replace(/\/+$/, '').split('/').pop() ?? '';
	const words = name
		.replace(/\.[^.]+$/, '')
		.replace(/[-_]+/g, ' ')
		.trim();
	return words ? words.charAt(0).toUpperCase() + words.slice(1) : 'Memory';
}

/** Where a note's card is on the Memory page. */
export function memoryAnchor(path: string): string {
	return `memory-${path.replace(/[^a-zA-Z0-9]+/g, '-')}`;
}
