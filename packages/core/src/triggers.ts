import { randomBytes, randomUUID } from 'node:crypto';
import { Cron } from 'croner';
import { and, asc, desc, eq, inArray, lt, lte, notInArray } from 'drizzle-orm';
import { EFFORTS, type Effort } from './anthropic.ts';
import { DEFAULT_PORT, readConfig } from './config.ts';
import { getDb } from './db/index.ts';
import { trigger, triggerRun } from './db/schema.ts';
import { getConversation } from './conversations.ts';
import { getPreset, listPresets, type Preset } from './presets.ts';

export type Trigger = typeof trigger.$inferSelect;
export type TriggerRun = typeof triggerRun.$inferSelect;
export type RunSource = TriggerRun['source'];
export type RunStatus = TriggerRun['status'];

export type TriggerWhen =
	{ kind: 'cron'; cron: string } | { kind: 'once'; runAt: Date } | { kind: 'webhook' };

export type TriggerWhat =
	{ action: 'agent'; prompt: string } | { action: 'script'; command: string };

/** A background run whose final reply ends with this line doesn't produce a notification. */
export const SILENT_REPLY = 'NO_NOTIFICATION';

const RUNS_KEPT_PER_TRIGGER = 100;
/** Runs one trigger (or one profile's `btw wake` calls) may have waiting; stops runaway scripts. */
export const MAX_PENDING_RUNS = 10;

// --- time ---

function cronJob(expr: string): Cron {
	return new Cron(expr, { paused: true, mode: '5-part' });
}

/** Next time a 5-field cron expression (in the gateway's local time zone) matches after `from`. */
export function nextCronRun(expr: string, from: Date = new Date()): Date | null {
	return cronJob(expr).nextRun(from);
}

/** Up to `limit` times a cron expression matches after `from` and before `until`. */
export function cronRunsBetween(expr: string, from: Date, until: Date, limit: number): Date[] {
	return cronJob(expr)
		.nextRuns(limit, from)
		.filter((date) => date < until);
}

/** "2026-09-26 17:00" (local time), an ISO timestamp, or a delay from now like "30m", "2h", "1d". */
export function parseRunAt(text: string, now: Date = new Date()): Date {
	const trimmed = text.trim();
	const delay = /^(\d+)\s*(m|min|h|d)$/i.exec(trimmed);
	if (delay) {
		const unit = { m: 60_000, min: 60_000, h: 3_600_000, d: 86_400_000 }[
			delay[2].toLowerCase() as 'm' | 'min' | 'h' | 'd'
		];
		return new Date(now.getTime() + Number(delay[1]) * unit);
	}
	const date = new Date(
		/^\d{4}-\d{2}-\d{2} \d/.test(trimmed) ? trimmed.replace(' ', 'T') : trimmed
	);
	if (Number.isNaN(date.getTime())) {
		throw new Error(`Can't read the time "${text}". Use "YYYY-MM-DD HH:MM" or a delay like 30m.`);
	}
	return date;
}

/** Local time, the same everywhere it's shown: "Thu, 25 Sept 2026, 08:00 CEST". */
export function formatLocalTime(date: Date): string {
	return date.toLocaleString('en-GB', {
		weekday: 'short',
		day: 'numeric',
		month: 'short',
		year: 'numeric',
		hour: '2-digit',
		minute: '2-digit',
		timeZoneName: 'short'
	});
}

export function describeWhen(t: Trigger): string {
	if (t.kind === 'cron') return `cron ${t.cron}`;
	if (t.kind === 'once') return `once at ${t.runAt ? formatLocalTime(t.runAt) : '?'}`;
	return 'webhook';
}

/** A one-time trigger that has fired at (or after) its time. Running it by hand earlier doesn't count. */
export function isFinished(t: Pick<Trigger, 'kind' | 'runAt' | 'lastRunAt'>): boolean {
	return t.kind === 'once' && !!t.runAt && !!t.lastRunAt && t.lastRunAt >= t.runAt;
}

function scheduleFor(t: Pick<Trigger, 'kind' | 'cron' | 'runAt' | 'enabled'>): Date | null {
	if (!t.enabled) return null;
	if (t.kind === 'cron' && t.cron) return nextCronRun(t.cron);
	if (t.kind === 'once') return t.runAt;
	return null;
}

// --- triggers ---

function checkWhen(when: TriggerWhen): void {
	if (when.kind === 'cron') {
		try {
			cronJob(when.cron);
		} catch (err) {
			throw new Error(
				`Invalid cron expression "${when.cron}" (use 5 fields: minute hour day month weekday): ${err instanceof Error ? err.message : err}`,
				{ cause: err }
			);
		}
	} else if (when.kind === 'once' && when.runAt.getTime() < Date.now() - 60_000) {
		throw new Error(`${formatLocalTime(when.runAt)} is in the past.`);
	}
}

function checkWhat(what: TriggerWhat): void {
	if (what.action === 'agent' && !what.prompt.trim()) throw new Error('The prompt is empty.');
	if (what.action === 'script' && !what.command.trim()) throw new Error('The command is empty.');
}

function checkName(profileId: string, name: string, exceptId?: string): string {
	const trimmed = name.trim();
	if (!trimmed) throw new Error('A trigger needs a name.');
	const clash = listTriggers(profileId).find(
		(t) => t.id !== exceptId && t.name.toLowerCase() === trimmed.toLowerCase()
	);
	if (clash) throw new Error(`This profile already has a trigger named "${clash.name}".`);
	return trimmed;
}

/** Null when empty. The Automations page falls back to a clock icon for names Lucide doesn't have. */
function checkIcon(icon: string | null | undefined): string | null {
	const name = icon?.trim().toLowerCase();
	if (!name) return null;
	if (!/^[a-z0-9-]{1,64}$/.test(name)) {
		throw new Error(`"${icon}" isn't a Lucide icon name. Use one like umbrella or cloud-rain.`);
	}
	return name;
}

function checkEffort(effort: Effort | undefined): Effort {
	if (effort === undefined) return 'medium';
	if (!EFFORTS.includes(effort)) throw new Error(`Effort must be one of ${EFFORTS.join(', ')}.`);
	return effort;
}

function whenColumns(when: TriggerWhen) {
	return {
		kind: when.kind,
		cron: when.kind === 'cron' ? when.cron.trim() : null,
		runAt: when.kind === 'once' ? when.runAt : null
	};
}

function whatColumns(what: TriggerWhat) {
	return {
		action: what.action,
		prompt: what.action === 'agent' ? what.prompt.trim() : null,
		command: what.action === 'script' ? what.command.trim() : null
	};
}

export function createTrigger(input: {
	profileId: string;
	name: string;
	summary?: string | null;
	icon?: string | null;
	when: TriggerWhen;
	what: TriggerWhat;
	presetId?: string | null;
	effort?: Effort;
	createdBy?: string | null;
}): Trigger {
	checkWhen(input.when);
	checkWhat(input.what);
	const row = {
		id: randomUUID(),
		profileId: input.profileId,
		name: checkName(input.profileId, input.name),
		summary: input.summary?.trim() || null,
		icon: checkIcon(input.icon),
		...whenColumns(input.when),
		webhookToken: input.when.kind === 'webhook' ? randomBytes(24).toString('base64url') : null,
		...whatColumns(input.what),
		presetId: input.presetId ?? null,
		effort: checkEffort(input.effort),
		enabled: true,
		lastRunAt: null,
		createdBy: input.createdBy ?? null,
		createdAt: new Date()
	};
	return getDb()
		.insert(trigger)
		.values({ ...row, nextRunAt: scheduleFor(row) })
		.returning()
		.get();
}

export function updateTrigger(
	id: string,
	patch: {
		name?: string;
		summary?: string | null;
		icon?: string | null;
		when?: TriggerWhen;
		what?: TriggerWhat;
		presetId?: string | null;
		effort?: Effort;
	}
): Trigger {
	const current = getTrigger(id);
	if (!current) throw new Error('No such trigger.');
	if (patch.when) checkWhen(patch.when);
	if (patch.what) checkWhat(patch.what);
	const next = {
		...current,
		...(patch.name !== undefined ? { name: checkName(current.profileId, patch.name, id) } : {}),
		...(patch.summary !== undefined ? { summary: patch.summary?.trim() || null } : {}),
		...(patch.icon !== undefined ? { icon: checkIcon(patch.icon) } : {}),
		...(patch.when ? whenColumns(patch.when) : {}),
		...(patch.what ? whatColumns(patch.what) : {}),
		...(patch.presetId !== undefined ? { presetId: patch.presetId } : {}),
		...(patch.effort !== undefined ? { effort: checkEffort(patch.effort) } : {})
	};
	if (patch.when) {
		next.webhookToken =
			patch.when.kind === 'webhook'
				? (current.webhookToken ?? randomBytes(24).toString('base64url'))
				: null;
	}
	// A one-time trigger that already fired is armed again by giving it a new time.
	if (patch.when?.kind === 'once' && current.kind === 'once' && !current.enabled) {
		next.enabled = true;
	}
	return getDb()
		.update(trigger)
		.set({ ...next, nextRunAt: scheduleFor(next) })
		.where(eq(trigger.id, id))
		.returning()
		.get();
}

export function setTriggerEnabled(id: string, enabled: boolean): Trigger {
	const current = getTrigger(id);
	if (!current) throw new Error('No such trigger.');
	if (enabled && isFinished(current)) {
		throw new Error('This one-time trigger already ran. Give it a new time to run it again.');
	}
	return getDb()
		.update(trigger)
		.set({ enabled, nextRunAt: scheduleFor({ ...current, enabled }) })
		.where(eq(trigger.id, id))
		.returning()
		.get();
}

export function deleteTrigger(id: string): void {
	getDb().delete(trigger).where(eq(trigger.id, id)).run();
}

export function getTrigger(id: string): Trigger | undefined {
	return getDb().select().from(trigger).where(eq(trigger.id, id)).get();
}

export function getTriggerByToken(token: string): Trigger | undefined {
	return getDb().select().from(trigger).where(eq(trigger.webhookToken, token)).get();
}

export function listTriggers(profileId?: string): Trigger[] {
	const query = getDb().select().from(trigger);
	return (profileId ? query.where(eq(trigger.profileId, profileId)) : query)
		.orderBy(asc(trigger.createdAt))
		.all();
}

/** By id, unique id prefix (as `btw trigger list` shows it) or name. */
export function findTrigger(ref: string, profileId?: string): Trigger {
	const candidates = listTriggers(profileId);
	const needle = ref.trim().toLowerCase();
	const exact = candidates.find((t) => t.id === ref);
	if (exact) return exact;
	const matches = [
		...new Set([
			...(needle.length >= 4 ? candidates.filter((t) => t.id.startsWith(needle)) : []),
			...candidates.filter((t) => t.name.toLowerCase() === needle)
		])
	];
	if (matches.length === 1) return matches[0];
	if (matches.length > 1) throw new Error(`"${ref}" matches more than one trigger; use its id.`);
	throw new Error(`No trigger "${ref}". See \`btw trigger list\`.`);
}

export function webhookUrl(token: string): string {
	const config = readConfig();
	const origin =
		process.env.ORIGIN || config.origin || `http://localhost:${config.port ?? DEFAULT_PORT}`;
	return `${origin.replace(/\/+$/, '')}/api/hooks/${token}`;
}

/**
 * The preset a trigger created from a conversation should use: the one given, else that
 * conversation's, else the first preset.
 */
export function resolvePreset(ref?: string, conversationId?: string): Preset | undefined {
	const presets = listPresets();
	if (ref) {
		const needle = ref.toLowerCase();
		const found = presets.find((p) => p.id === ref || p.name.toLowerCase() === needle);
		if (!found) throw new Error(`No model preset "${ref}". See \`btw preset list\`.`);
		return found;
	}
	const fromConversation = conversationId ? getConversation(conversationId)?.presetId : null;
	return (fromConversation && getPreset(fromConversation)) || presets[0];
}

/** Triggers whose scheduled time has come. Webhook triggers are never scheduled. */
export function dueTriggers(now: Date): Trigger[] {
	return getDb().select().from(trigger).where(lte(trigger.nextRunAt, now)).all();
}

/** Records a scheduled firing and moves the schedule on. Missed firings collapse into this one. */
export function markFired(t: Trigger, now: Date): void {
	const enabled = t.kind === 'once' ? false : t.enabled;
	getDb()
		.update(trigger)
		.set({
			lastRunAt: now,
			enabled,
			nextRunAt: t.kind === 'cron' && enabled && t.cron ? nextCronRun(t.cron, now) : null
		})
		.where(eq(trigger.id, t.id))
		.run();
}

// --- runs ---

/** Queues a run of the trigger; the gateway's scheduler starts it within a few seconds. */
function checkPending(profileId: string, triggerId: string | null): void {
	const waiting = getDb()
		.select({ id: triggerRun.id })
		.from(triggerRun)
		.where(
			and(
				eq(triggerRun.status, 'pending'),
				triggerId ? eq(triggerRun.triggerId, triggerId) : eq(triggerRun.profileId, profileId)
			)
		)
		.all().length;
	if (waiting >= MAX_PENDING_RUNS) {
		throw new Error(`${waiting} runs are already waiting to start; not queueing another.`);
	}
}

export function queueRun(t: Trigger, source: RunSource, payload?: string | null): TriggerRun {
	checkPending(t.profileId, t.id);
	if (source !== 'cron' && source !== 'once') {
		getDb().update(trigger).set({ lastRunAt: new Date() }).where(eq(trigger.id, t.id)).run();
	}
	return insertRun({
		profileId: t.profileId,
		triggerId: t.id,
		title: t.name,
		action: t.action,
		source,
		prompt: t.action === 'agent' ? t.prompt : null,
		payload: payload ?? null,
		presetId: t.presetId,
		effort: t.effort
	});
}

/** `btw wake`: queues a background agent run, usually from a trigger's script. */
export function queueWake(input: {
	profileId: string;
	text: string;
	triggerId?: string | null;
	title?: string;
	presetId?: string | null;
	effort?: Effort;
}): TriggerRun {
	if (!input.text.trim()) throw new Error('Say what the agent should do.');
	const t = input.triggerId ? getTrigger(input.triggerId) : undefined;
	checkPending(t?.profileId ?? input.profileId, t?.id ?? null);
	return insertRun({
		profileId: t?.profileId ?? input.profileId,
		triggerId: t?.id ?? null,
		title: input.title?.trim() || t?.name || 'btw',
		action: 'agent',
		source: 'wake',
		prompt: input.text.trim(),
		payload: null,
		presetId: input.presetId ?? t?.presetId ?? null,
		effort: checkEffort(input.effort ?? t?.effort)
	});
}

function insertRun(
	values: Omit<
		TriggerRun,
		'id' | 'status' | 'conversationId' | 'output' | 'createdAt' | 'startedAt' | 'finishedAt'
	>
): TriggerRun {
	return getDb()
		.insert(triggerRun)
		.values({ ...values, id: randomUUID(), status: 'pending', createdAt: new Date() })
		.returning()
		.get();
}

export function getRun(id: string): TriggerRun | undefined {
	return getDb().select().from(triggerRun).where(eq(triggerRun.id, id)).get();
}

export function listRuns(triggerId: string, limit = 10): TriggerRun[] {
	return getDb()
		.select()
		.from(triggerRun)
		.where(eq(triggerRun.triggerId, triggerId))
		.orderBy(desc(triggerRun.createdAt))
		.limit(limit)
		.all();
}

export function runsWithStatus(...statuses: RunStatus[]): TriggerRun[] {
	return getDb()
		.select()
		.from(triggerRun)
		.where(inArray(triggerRun.status, statuses))
		.orderBy(asc(triggerRun.createdAt))
		.all();
}

/** A trigger doesn't start a scheduled run while its previous one of the same kind is unfinished. */
export function hasActiveRun(triggerId: string, action: TriggerRun['action']): boolean {
	return !!getDb()
		.select({ id: triggerRun.id })
		.from(triggerRun)
		.where(
			and(
				eq(triggerRun.triggerId, triggerId),
				eq(triggerRun.action, action),
				inArray(triggerRun.status, ['pending', 'running'])
			)
		)
		.get();
}

export function runningRunFor(conversationId: string): TriggerRun | undefined {
	return getDb()
		.select()
		.from(triggerRun)
		.where(and(eq(triggerRun.conversationId, conversationId), eq(triggerRun.status, 'running')))
		.get();
}

export function lastFinishedRun(triggerId: string, action: TriggerRun['action']) {
	return getDb()
		.select()
		.from(triggerRun)
		.where(
			and(
				eq(triggerRun.triggerId, triggerId),
				eq(triggerRun.action, action),
				notInArray(triggerRun.status, ['pending', 'running'])
			)
		)
		.orderBy(desc(triggerRun.finishedAt))
		.limit(1)
		.get();
}

export function updateRun(
	id: string,
	patch: Partial<Pick<TriggerRun, 'status' | 'conversationId' | 'output'>>
): void {
	const now = new Date();
	const times =
		patch.status === 'running'
			? { startedAt: now }
			: patch.status && patch.status !== 'pending'
				? { finishedAt: now }
				: {};
	getDb()
		.update(triggerRun)
		.set({ ...patch, ...times })
		.where(eq(triggerRun.id, id))
		.run();
}

/** Drops finished runs older than `before`, and all but the newest ones of each trigger. */
export function pruneRuns(before: Date): void {
	const db = getDb();
	const finished = notInArray(triggerRun.status, ['pending', 'running']);
	db.delete(triggerRun)
		.where(and(finished, lt(triggerRun.createdAt, before)))
		.run();
	for (const t of listTriggers()) {
		const cutoff = db
			.select({ createdAt: triggerRun.createdAt })
			.from(triggerRun)
			.where(eq(triggerRun.triggerId, t.id))
			.orderBy(desc(triggerRun.createdAt))
			.limit(1)
			.offset(RUNS_KEPT_PER_TRIGGER)
			.get();
		if (cutoff) {
			db.delete(triggerRun)
				.where(
					and(eq(triggerRun.triggerId, t.id), finished, lte(triggerRun.createdAt, cutoff.createdAt))
				)
				.run();
		}
	}
}

/** The first message of a background run: what happened, what to do, and how to answer. */
export function runMessage(run: TriggerRun): string {
	const via = {
		cron: 'on its schedule',
		once: 'one-time',
		webhook: 'its webhook was called',
		wake: 'woken by a script',
		manual: 'started by hand'
	}[run.source];
	const parts = [
		`[Automation "${run.title}" · ${via} · ${formatLocalTime(run.createdAt)}]`,
		run.prompt ?? ''
	];
	if (run.payload) {
		parts.push(`The webhook request body:\n<payload>\n${run.payload}\n</payload>`);
	}
	parts.push(
		`Nobody is watching this run. Your final reply is shown as a notification to everyone in this profile, and they can open it to continue the conversation with you, so make it short and self-contained. If there is nothing worth telling them, reply with only ${SILENT_REPLY}.`
	);
	return parts.join('\n\n');
}

/** True when the reply's last line is the silent marker (ignoring markdown around it). */
export function isSilentReply(text: string): boolean {
	const last = text.trim().split('\n').at(-1) ?? '';
	return last.replace(/[^A-Za-z_]/g, '') === SILENT_REPLY;
}
