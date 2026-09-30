import { mkdirSync } from 'node:fs';
import { hasBackgroundCommands } from './background.ts';
import {
	appendRow,
	createConversation,
	deleteHiddenConversations,
	getConversation,
	lastCommittedRow,
	replyText
} from './conversations.ts';
import { pruneUploads } from './attachments.ts';
import { pruneMedia } from './media.ts';
import { startLearning } from './memory-learning.ts';
import { profileCards } from './memory-cards.ts';
import { recallByWords, startEmbeddingMemory } from './memory-search.ts';
import { pruneProviderFiles } from './provider-files.ts';
import { createNotification, pruneNotifications } from './notifications.ts';
import { profileDir } from './paths.ts';
import { getDefaultPreset, getPreset } from './presets.ts';
import { getProfile, listProfiles, noticeProfileChanges } from './profiles.ts';
import { commandEnv, runCommand, type RunCommandResult } from './run-command.ts';
import { kick, onLoopEnd } from './runner.ts';
import { processSubagents, startSubagentHost } from './subagent-host.ts';
import {
	describeWhen,
	dueTriggers,
	getTrigger,
	getTriggerByToken,
	hasActiveRun,
	isSilentReply,
	lastFinishedRun,
	markFired,
	pruneRuns,
	queueRun,
	runMessage,
	runningRunFor,
	runsWithStatus,
	setTriggerEnabled,
	updateRun,
	type Trigger,
	type TriggerRun
} from './triggers.ts';

const TICK_MS = 5_000;
const PRUNE_EVERY_MS = 60 * 60_000;
/** Background agent runs going at once; the rest wait their turn. */
const MAX_AGENT_RUNS = 3;
/** Finished runs, notifications and background runs nobody continued are deleted after this. */
const KEEP_MS = 30 * 24 * 60 * 60_000;
const SCRIPT_TIMEOUT_SECONDS = 600;
const SCRIPT_OUTPUT_CHARS = 4_000;
export const MAX_PAYLOAD_BYTES = 64 * 1024;

const holder = globalThis as unknown as { __noluneScheduler?: boolean };

/**
 * Gateway only. Every few seconds: fires triggers that are due and starts queued runs (including
 * the ones `nolune wake` and `nolune trigger run` queue from other processes), starts the subagents
 * that `nolune agent` asks for, and notices profiles that `nolune profile` changed from another process.
 * Chats that went quiet get looked over for memory (memory-learning.ts), and memory facts get
 * their embeddings (memory-search.ts).
 */
export function startScheduler(): void {
	if (holder.__noluneScheduler) return;
	holder.__noluneScheduler = true;
	onLoopEnd(finishAgentRun);
	startSubagentHost();
	startLearning();
	startEmbeddingMemory(listProfiles().map((p) => p.slug));
	recoverRuns();
	prune();
	tick();
	setInterval(tick, TICK_MS).unref();
	setInterval(prune, PRUNE_EVERY_MS).unref();
}

function tick(): void {
	try {
		fireDueTriggers(new Date());
		processQueue();
		processSubagents();
		noticeProfileChanges();
	} catch (err) {
		console.error('[nolune] scheduler tick failed:', err);
	}
}

function fireDueTriggers(now: Date): void {
	for (const t of dueTriggers(now)) {
		try {
			markFired(t, now);
		} catch (err) {
			// Otherwise it would be due again on every tick.
			console.error(`[nolune] trigger "${t.name}" could not be scheduled and was paused:`, err);
			setTriggerEnabled(t.id, false);
			continue;
		}
		if (hasActiveRun(t.id, t.action)) {
			console.log(`[nolune] trigger "${t.name}": the previous run is still going, skipped`);
			continue;
		}
		try {
			queueRun(t, t.kind === 'once' ? 'once' : 'cron');
		} catch (err) {
			console.error(
				`[nolune] trigger "${t.name}" skipped:`,
				err instanceof Error ? err.message : err
			);
		}
	}
}

/** Starts queued runs: agent runs up to the limit, and one script run per trigger at a time. */
export function processQueue(): void {
	const running = runsWithStatus('running');
	let agents = running.filter((r) => r.action === 'agent').length;
	const busyScripts = new Set(running.filter((r) => r.action === 'script').map((r) => r.triggerId));
	for (const run of runsWithStatus('pending')) {
		try {
			if (run.action === 'agent') {
				if (agents >= MAX_AGENT_RUNS) continue;
				agents++;
				startAgentRun(run);
			} else {
				if (busyScripts.has(run.triggerId)) continue;
				busyScripts.add(run.triggerId);
				startScriptRun(run);
			}
		} catch (err) {
			console.error(`[nolune] run "${run.title}" could not start:`, err);
			failRun(run, `Could not start: ${err instanceof Error ? err.message : String(err)}`);
		}
	}
}

export function fireWebhook(
	token: string,
	payload: string
): TriggerRun | 'not_found' | 'paused' | 'busy' {
	const t = getTriggerByToken(token);
	if (!t || t.kind !== 'webhook') return 'not_found';
	if (!t.enabled) return 'paused';
	let run: TriggerRun;
	try {
		run = queueRun(t, 'webhook', payload || null);
	} catch {
		return 'busy';
	}
	processQueue();
	return run;
}

export function runTriggerNow(t: Trigger): TriggerRun {
	const run = queueRun(t, 'manual');
	processQueue();
	return run;
}

function failRun(run: TriggerRun, message: string, conversationId: string | null = null): void {
	updateRun(run.id, { status: 'failed', output: message });
	createNotification({
		profileId: run.profileId,
		triggerId: run.triggerId,
		conversationId,
		level: 'error',
		title: `${run.title} failed`,
		body: message
	});
}

function startAgentRun(run: TriggerRun): void {
	const profile = getProfile(run.profileId);
	if (!profile) {
		updateRun(run.id, { status: 'failed', output: 'The profile no longer exists.' });
		return;
	}
	const preset = (run.presetId && getPreset(run.presetId)) || getDefaultPreset();
	if (!preset) {
		failRun(run, 'No models are set up yet. An admin can add one on the Models page.');
		return;
	}
	const conv = createConversation({
		profile,
		presetId: preset.id,
		userId: null,
		effort: run.effort,
		title: run.title,
		hidden: true
	});
	const text = [run.prompt, run.payload].filter(Boolean).join('\n\n');
	const memory = recall(profile, text, conv.systemPrompt);
	appendRow({
		conversationId: conv.id,
		role: 'user',
		kind: 'trigger',
		senderName: run.title,
		text,
		blocks: [
			{ type: 'text', text: runMessage(run) },
			...(memory ? [{ type: 'text' as const, text: memory }] : [])
		]
	});
	updateRun(run.id, { status: 'running', conversationId: conv.id });
	console.log(`[nolune] background run "${run.title}" started in ${conv.id.slice(0, 8)}`);
	kick(conv.id);
}

/**
 * What memory has on an automation's prompt, like on a person's message but by words only: the
 * run starts now. Never stops the run.
 */
function recall(profile: { id: string; slug: string }, text: string, known: string): string | null {
	try {
		const cards = profileCards(profile.id).map((card) => card.path);
		return recallByWords(profile.slug, text, { known, cards });
	} catch (err) {
		console.error(`[nolune] ${profile.slug} could not look in memory for a background run:`, err);
		return null;
	}
}

/** Runs when a background run's agent loop stops: its final reply becomes the notification. */
function finishAgentRun(conversationId: string, error: string | null): void {
	const run = runningRunFor(conversationId);
	if (!run) return;
	// Not over while commands run in its background (a subagent it waits for, say): their output
	// starts it again, and its reply after that is the one worth a notification.
	if (!error && hasBackgroundCommands(conversationId)) return;
	if (error) {
		failRun(run, error, conversationId);
		return;
	}
	const last = lastCommittedRow(conversationId);
	if (last?.kind !== 'assistant') {
		// Someone opened the run and pressed Stop.
		updateRun(run.id, { status: 'stopped' });
		return;
	}
	if (getConversation(conversationId)?.hidden === false) {
		// Someone continued it in chat while it ran; they already see the reply.
		updateRun(run.id, { status: 'ok' });
		return;
	}
	const text = replyText(last);
	if (!text || isSilentReply(text)) {
		updateRun(run.id, { status: 'silent' });
		return;
	}
	updateRun(run.id, { status: 'notified' });
	createNotification({
		profileId: run.profileId,
		triggerId: run.triggerId,
		conversationId,
		title: run.title,
		body: text
	});
}

function startScriptRun(run: TriggerRun): void {
	const t = run.triggerId ? getTrigger(run.triggerId) : undefined;
	const profile = getProfile(run.profileId);
	if (!t?.command || !profile) {
		updateRun(run.id, { status: 'failed', output: 'The trigger no longer exists.' });
		return;
	}
	const dir = profileDir(profile.slug);
	mkdirSync(dir, { recursive: true });
	updateRun(run.id, { status: 'running' });
	runCommand(
		{ command: t.command, timeoutSeconds: SCRIPT_TIMEOUT_SECONDS },
		{
			defaultCwd: dir,
			env: commandEnv({
				NOLUNE_PROFILE: profile.slug,
				NOLUNE_PROFILE_DIR: dir,
				NOLUNE_TRIGGER_ID: t.id,
				...(run.payload ? { NOLUNE_PAYLOAD: run.payload } : {})
			}),
			signal: new AbortController().signal,
			abortReason: () => 'Stopped.'
		}
	)
		.catch((err): RunCommandResult => ({
			content: `Failed to run: ${err instanceof Error ? err.message : String(err)}`,
			isError: true,
			exitCode: null
		}))
		.then((result) => finishScriptRun(run, t, result));
}

/** Script failures notify once, when a working script starts failing, not on every run. */
function finishScriptRun(run: TriggerRun, t: Trigger, result: RunCommandResult): void {
	const ok = result.exitCode === 0;
	const output =
		result.content.length > SCRIPT_OUTPUT_CHARS
			? `…${result.content.slice(-SCRIPT_OUTPUT_CHARS)}`
			: result.content;
	const previous = lastFinishedRun(t.id, 'script');
	updateRun(run.id, { status: ok ? 'ok' : 'failed', output });
	if (ok || previous?.status === 'failed') return;
	createNotification({
		profileId: run.profileId,
		triggerId: t.id,
		level: 'error',
		title: `${t.name}: script failed`,
		body: `The script of the automation "${t.name}" (${describeWhen(t)}, id ${t.id.slice(0, 8)}) failed. It keeps running on schedule; you'll hear about it again only if it recovers and then fails.\n\nCommand: ${t.command}\n\n${output}`
	});
}

/** After a restart: background agent runs pick up where they stopped; scripts are marked failed. */
function recoverRuns(): void {
	for (const run of runsWithStatus('running')) {
		if (run.action === 'script') {
			updateRun(run.id, {
				status: 'failed',
				output: 'Interrupted: the gateway restarted while the script was running.'
			});
		} else if (run.conversationId && getConversation(run.conversationId)) {
			kick(run.conversationId);
		} else {
			updateRun(run.id, { status: 'failed', output: 'Its conversation was deleted.' });
		}
	}
}

function prune(): void {
	try {
		const before = new Date(Date.now() - KEEP_MS);
		pruneRuns(before);
		pruneNotifications(before);
		deleteHiddenConversations(before);
		pruneUploads();
		pruneMedia();
	} catch (err) {
		console.error('[nolune] pruning old runs failed:', err);
	}
	pruneProviderFiles().catch((err) => {
		console.error('[nolune] pruning uploaded files failed:', err);
	});
}
