/** The command's first line, for the compact technical label. */
export function firstLine(command: string, max = 120): string {
	const line = command.trim().split('\n')[0];
	return line.length > max ? line.slice(0, max) + '…' : line;
}

/** Read in core, which builds the transcript with it. */
export { partialToolInput, type ToolInput } from '@nolune/core/transcript';
