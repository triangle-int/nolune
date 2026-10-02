import { idleCompactionMinutes } from './compaction.ts';
import { lastCommittedRow, recentConversationIds } from './conversations.ts';
import { compactIdle, onLoopEnd, onRunningChange } from './runner.ts';

/*
 * Quiet chats get summarized (compaction.ts), when Models & keys says after how many minutes
 * (compactWhenIdle): the next reply then reads the summary rather than the whole chat. Under an
 * hour, the summary itself reads the chat from the prompt cache, which costs little. Each chat
 * gets a timer when the agent's turn ends, which a new turn cancels; the gateway starts them again
 * for chats that were quiet less long than that when it starts.
 */

const holder = globalThis as unknown as {
	__noluneIdleCompaction?: boolean;
	__noluneIdleTimers?: Map<string, ReturnType<typeof setTimeout>>;
};
const timers = (holder.__noluneIdleTimers ??= new Map());

function cancel(conversationId: string): void {
	clearTimeout(timers.get(conversationId));
	timers.delete(conversationId);
}

/**
 * Counts the chat's quiet minutes from its latest message, and summarizes it once there are
 * enough, if it's still quiet and still worth it (compactIdle). Nothing while quiet chats aren't.
 */
function watch(conversationId: string): void {
	cancel(conversationId);
	const minutes = idleCompactionMinutes();
	if (!minutes) return;
	const last = lastCommittedRow(conversationId);
	if (!last) return;
	const due = last.createdAt.getTime() + minutes * 60_000 - Date.now();
	const timer = setTimeout(
		() => {
			timers.delete(conversationId);
			const now = idleCompactionMinutes();
			// Changed meanwhile, or the chat wasn't quiet after all: count again.
			if (now !== minutes || lastCommittedRow(conversationId)?.id !== last.id) {
				watch(conversationId);
				return;
			}
			compactIdle(conversationId);
		},
		Math.max(0, due)
	);
	timer.unref();
	timers.set(conversationId, timer);
}

/** Watches the chats active within the setting's minutes, as it is now. */
function watchQuietChats(): void {
	for (const id of [...timers.keys()]) cancel(id);
	const minutes = idleCompactionMinutes();
	if (!minutes) return;
	for (const id of recentConversationIds(new Date(Date.now() - minutes * 60_000))) watch(id);
}

/** After the setting changed: in the gateway, chats are watched as it says now. */
export function idleCompactionChanged(): void {
	if (holder.__noluneIdleCompaction) watchQuietChats();
}

/** Gateway only. */
export function startIdleCompaction(): void {
	if (holder.__noluneIdleCompaction) return;
	holder.__noluneIdleCompaction = true;
	onRunningChange((conversationId, running) => {
		if (running) cancel(conversationId);
	});
	onLoopEnd((conversationId) => watch(conversationId));
	watchQuietChats();
}
