import type { Messages } from './i18n';
import { firstLine } from './commands';
import type { ActivityPart, ToolResult } from '@nolune/core/transcript';

/** Built in core (packages/core/src/transcript.ts), so apps of their own get the same. */
export * from '@nolune/core/transcript';

/**
 * What work in progress is doing, as its collapsed group says it: the summary of the command that
 * is running, summarizing the conversation, or Thinking. The live avatar shows the same on hover.
 */
export function activeStepLabel(
	part: ActivityPart,
	results: Record<string, ToolResult>,
	technical: boolean,
	m: Messages
): string {
	const last = part.steps.at(-1);
	if (last?.type === 'compaction' && !last.summary) return m.steps.summarizing;
	if (last?.type === 'command' && !results[last.id]) {
		if (technical && last.command) return m.steps.running(firstLine(last.command, 80));
		return last.summary ?? m.steps.runningACommand;
	}
	return m.steps.thinking;
}

export function formatDuration(ms: number, m: Messages): string | null {
	const seconds = Math.round(ms / 1000);
	if (seconds < 1) return null;
	if (seconds < 60) return m.time.seconds(seconds);
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return m.time.minutesSeconds(minutes, seconds % 60);
	return m.time.hoursMinutes(Math.floor(minutes / 60), minutes % 60);
}
