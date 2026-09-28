import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { writeConfig } from './config.ts';
import { createConversation } from './conversations.ts';
import { setDefaultPreset } from './presets.ts';
import type { Profile } from './profiles.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';
import {
	createTrigger,
	deleteTrigger,
	dueTriggers,
	findTrigger,
	getRun,
	getTrigger,
	getTriggerByToken,
	hasActiveRun,
	isFinished,
	isSilentReply,
	lastFinishedRun,
	listRuns,
	listTriggers,
	markFired,
	MAX_PENDING_RUNS,
	parseRunAt,
	pruneRuns,
	queueRun,
	queueWake,
	resolvePreset,
	runMessage,
	runningRunFor,
	setTriggerEnabled,
	updateRun,
	updateTrigger,
	webhookUrl
} from './triggers.ts';

/** Local time, so the cron expressions below mean the same in every time zone. */
const at = (day: number, hour: number, minute = 0) => new Date(2026, 8, day, hour, minute);

let profile: Profile;

beforeEach(() => {
	vi.useFakeTimers({ toFake: ['Date'] });
	vi.setSystemTime(at(28, 6));
	profile = makeFamily().profile;
});

afterEach(() => {
	vi.useRealTimers();
});

const umbrellas = { action: 'agent', prompt: 'Do we need umbrellas today?' } as const;

function daily(name = 'Umbrellas') {
	return createTrigger({
		profileId: profile.id,
		name,
		when: { kind: 'cron', cron: '30 7 * * *' },
		what: umbrellas
	});
}

describe('createTrigger', () => {
	it('schedules cron and one-time triggers, and gives webhooks a token', () => {
		const cron = createTrigger({
			profileId: profile.id,
			name: ' Umbrellas ',
			summary: '  ',
			icon: ' Umbrella ',
			when: { kind: 'cron', cron: ' 30 7 * * * ' },
			what: { action: 'agent', prompt: ' Do we need umbrellas today? ' }
		});
		expect(cron).toMatchObject({
			name: 'Umbrellas',
			summary: null,
			icon: 'umbrella',
			cron: '30 7 * * *',
			prompt: 'Do we need umbrellas today?',
			command: null,
			effort: 'medium',
			enabled: true,
			webhookToken: null,
			nextRunAt: at(28, 7, 30)
		});

		const once = createTrigger({
			profileId: profile.id,
			name: 'Dentist',
			when: { kind: 'once', runAt: at(30, 9) },
			what: { action: 'script', command: 'say dentist' }
		});
		expect(once).toMatchObject({ runAt: at(30, 9), nextRunAt: at(30, 9), prompt: null });

		const hook = createTrigger({
			profileId: profile.id,
			name: 'School mail',
			when: { kind: 'webhook' },
			what: umbrellas
		});
		expect(hook.nextRunAt).toBeNull();
		expect(hook.webhookToken).toMatch(/^[\w-]{32}$/);
		expect(getTriggerByToken(hook.webhookToken!)?.id).toBe(hook.id);
	});

	it('refuses what it could not run', () => {
		const make = (overrides: Partial<Parameters<typeof createTrigger>[0]>) => () =>
			createTrigger({
				profileId: profile.id,
				name: 'Test',
				when: { kind: 'cron', cron: '0 7 * * *' },
				what: umbrellas,
				...overrides
			});
		expect(make({ when: { kind: 'cron', cron: '0 7 * *' } })).toThrow('Invalid cron expression');
		expect(make({ when: { kind: 'once', runAt: at(27, 6) } })).toThrow('is in the past');
		expect(make({ what: { action: 'agent', prompt: ' ' } })).toThrow('The prompt is empty.');
		expect(make({ what: { action: 'script', command: '' } })).toThrow('The command is empty.');
		expect(make({ name: ' ' })).toThrow('A trigger needs a name.');
		expect(make({ icon: 'cloud rain' })).toThrow(`isn't a Lucide icon name`);
		expect(listTriggers()).toEqual([]);
	});

	it('wants names unique within a profile, ignoring case', () => {
		daily('Umbrellas');
		expect(() => daily('UMBRELLAS')).toThrow('already has a trigger named "Umbrellas"');
		const other = makeFamily('Max').profile;
		expect(() =>
			createTrigger({
				profileId: other.id,
				name: 'Umbrellas',
				when: { kind: 'webhook' },
				what: umbrellas
			})
		).not.toThrow();
	});
});

describe('scheduling', () => {
	it('moves a cron trigger to its next time when it fires, skipping missed ones', () => {
		const t = daily();
		expect(dueTriggers(at(28, 7, 29))).toEqual([]);
		vi.setSystemTime(at(29, 12));
		expect(dueTriggers(new Date()).map((d) => d.id)).toEqual([t.id]);
		markFired(t, new Date());
		expect(getTrigger(t.id)).toMatchObject({ lastRunAt: at(29, 12), nextRunAt: at(30, 7, 30) });
	});

	it('turns a one-time trigger off once it fired, until it gets a new time', () => {
		const t = createTrigger({
			profileId: profile.id,
			name: 'Dentist',
			when: { kind: 'once', runAt: at(28, 9) },
			what: umbrellas
		});
		vi.setSystemTime(at(28, 9));
		markFired(t, new Date());
		const fired = getTrigger(t.id)!;
		expect(fired).toMatchObject({ enabled: false, nextRunAt: null });
		expect(isFinished(fired)).toBe(true);
		expect(() => setTriggerEnabled(t.id, true)).toThrow('already ran');

		const again = updateTrigger(t.id, { when: { kind: 'once', runAt: at(29, 9) } });
		expect(again).toMatchObject({ enabled: true, nextRunAt: at(29, 9) });
		expect(isFinished(again)).toBe(false);
	});

	it('unschedules a trigger while it is off', () => {
		const t = daily();
		expect(setTriggerEnabled(t.id, false)).toMatchObject({ enabled: false, nextRunAt: null });
		expect(dueTriggers(at(30, 12))).toEqual([]);
		expect(setTriggerEnabled(t.id, true).nextRunAt).toEqual(at(28, 7, 30));
	});

	it('keeps a webhook token while it stays a webhook', () => {
		const t = daily();
		const hook = updateTrigger(t.id, { when: { kind: 'webhook' } });
		expect(hook).toMatchObject({ kind: 'webhook', cron: null, nextRunAt: null });
		expect(updateTrigger(t.id, { name: 'Renamed', when: { kind: 'webhook' } }).webhookToken).toBe(
			hook.webhookToken
		);
		expect(updateTrigger(t.id, { when: { kind: 'cron', cron: '0 * * * *' } })).toMatchObject({
			webhookToken: null,
			nextRunAt: at(28, 7)
		});
	});
});

describe('findTrigger', () => {
	it('finds by id, id prefix or name', () => {
		const t = daily('Umbrellas');
		expect(findTrigger(t.id).id).toBe(t.id);
		expect(findTrigger(t.id.slice(0, 8)).id).toBe(t.id);
		expect(findTrigger('umbrellas', profile.id).id).toBe(t.id);
		expect(() => findTrigger(t.id.slice(0, 3))).toThrow('No trigger');
		expect(() => findTrigger('Umbrellas', makeFamily('Max').profile.id)).toThrow('No trigger');
	});

	it('refuses a reference that matches more than one', () => {
		const other = makeFamily('Max').profile;
		daily('Umbrellas');
		createTrigger({
			profileId: other.id,
			name: 'Umbrellas',
			when: { kind: 'webhook' },
			what: umbrellas
		});
		expect(() => findTrigger('Umbrellas')).toThrow('matches more than one trigger');
	});
});

describe('runs', () => {
	it('queues runs, records when they start and finish, and knows which is active', () => {
		const t = daily();
		const run = queueRun(t, 'manual');
		expect(run).toMatchObject({ status: 'pending', title: 'Umbrellas', prompt: umbrellas.prompt });
		expect(getTrigger(t.id)?.lastRunAt).toEqual(at(28, 6));
		expect(hasActiveRun(t.id, 'agent')).toBe(true);
		expect(hasActiveRun(t.id, 'script')).toBe(false);

		const chat = createConversation({
			profile,
			presetId: makePreset().id,
			userId: null,
			hidden: true
		});
		vi.setSystemTime(at(28, 6, 1));
		updateRun(run.id, { status: 'running', conversationId: chat.id });
		expect(runningRunFor(chat.id)?.id).toBe(run.id);
		vi.setSystemTime(at(28, 6, 2));
		updateRun(run.id, { status: 'notified' });

		expect(getRun(run.id)).toMatchObject({ startedAt: at(28, 6, 1), finishedAt: at(28, 6, 2) });
		expect(hasActiveRun(t.id, 'agent')).toBe(false);
		expect(runningRunFor(chat.id)).toBeUndefined();
		expect(lastFinishedRun(t.id, 'agent')?.id).toBe(run.id);
	});

	it("doesn't count a scheduled firing as a run by hand", () => {
		const t = daily();
		queueRun(t, 'cron');
		expect(getTrigger(t.id)?.lastRunAt).toBeNull();
	});

	it('stops queueing when too many runs of a trigger are waiting', () => {
		const t = daily();
		for (let i = 0; i < MAX_PENDING_RUNS; i++) queueRun(t, 'webhook', `{"n":${i}}`);
		expect(() => queueRun(t, 'webhook')).toThrow('10 runs are already waiting to start');
		updateRun(listRuns(t.id)[0].id, { status: 'running' });
		expect(() => queueRun(t, 'webhook')).not.toThrow();
	});

	it("stops queueing when too many of a profile's wakes are waiting", () => {
		for (let i = 0; i < MAX_PENDING_RUNS; i++) queueWake({ profileId: profile.id, text: 'hi' });
		expect(() => queueWake({ profileId: profile.id, text: 'hi' })).toThrow('already waiting');
		const other = makeFamily('Max').profile;
		expect(() => queueWake({ profileId: other.id, text: 'hi' })).not.toThrow();
	});

	it("doesn't count a trigger's waiting runs against the profile's wakes, or the other way round", () => {
		const t = daily();
		for (let i = 0; i < MAX_PENDING_RUNS; i++) queueRun(t, 'webhook');
		expect(() => queueWake({ profileId: profile.id, text: 'hi' })).not.toThrow();

		const other = daily('Bins');
		for (let i = 1; i < MAX_PENDING_RUNS; i++) queueWake({ profileId: profile.id, text: 'hi' });
		expect(() => queueRun(other, 'webhook')).not.toThrow();
		// A wake on behalf of a trigger counts toward that trigger's runs.
		expect(() => queueWake({ profileId: profile.id, text: 'hi', triggerId: t.id })).toThrow(
			'already waiting'
		);
	});

	it('wakes the agent in the name of a trigger, or on its own', () => {
		const t = daily();
		expect(
			queueWake({ profileId: profile.id, text: ' Mail from school ', triggerId: t.id })
		).toMatchObject({
			triggerId: t.id,
			title: 'Umbrellas',
			source: 'wake',
			action: 'agent',
			prompt: 'Mail from school'
		});
		expect(queueWake({ profileId: profile.id, text: 'hi' }).title).toBe('nolune');
		expect(() => queueWake({ profileId: profile.id, text: ' ' })).toThrow(
			'Say what the agent should do.'
		);
	});

	it('keeps the runs of a deleted trigger', () => {
		const t = daily();
		const run = queueRun(t, 'manual');
		deleteTrigger(t.id);
		expect(getRun(run.id)).toMatchObject({ triggerId: null, title: 'Umbrellas' });
	});
});

describe('pruneRuns', () => {
	it('drops old finished runs and keeps unfinished ones', () => {
		const t = daily();
		const finished = queueRun(t, 'manual');
		updateRun(finished.id, { status: 'ok' });
		const waiting = queueRun(t, 'manual');
		vi.setSystemTime(at(30, 6));
		const recent = queueRun(t, 'manual');
		updateRun(recent.id, { status: 'ok' });

		pruneRuns(at(29, 6));
		expect(listRuns(t.id).map((r) => r.id)).toEqual([recent.id, waiting.id]);
	});

	it('keeps the newest 100 runs of each trigger', () => {
		const t = daily();
		const waiting = queueRun(t, 'manual');
		const ids: string[] = [];
		for (let i = 0; i < 102; i++) {
			vi.advanceTimersByTime(1000);
			const run = queueRun(t, 'manual');
			updateRun(run.id, { status: 'silent' });
			ids.push(run.id);
		}
		pruneRuns(at(1, 0));
		const kept = listRuns(t.id, 200).map((r) => r.id);
		expect(kept).toHaveLength(101);
		expect(kept).toContain(waiting.id);
		expect(kept).not.toContain(ids[0]);
		expect(kept).not.toContain(ids[1]);
		expect(kept).toContain(ids[2]);
	});
});

describe('resolvePreset', () => {
	it("uses the preset named, else the conversation's, else the default", () => {
		const sonnet = makePreset('Sonnet');
		const opus = makePreset('Opus');
		const chat = createConversation({ profile, presetId: opus.id, userId: null });
		expect(resolvePreset('sonnet')?.id).toBe(sonnet.id);
		expect(resolvePreset(undefined, chat.id)?.id).toBe(opus.id);
		expect(resolvePreset()?.id).toBe(sonnet.id);
		setDefaultPreset('Opus');
		expect(resolvePreset()?.id).toBe(opus.id);
		expect(() => resolvePreset('Haiku')).toThrow('No model preset "Haiku"');
	});
});

describe('run text', () => {
	it('tells the agent what started the run and how to stay quiet', () => {
		const t = createTrigger({
			profileId: profile.id,
			name: 'School mail',
			when: { kind: 'webhook' },
			what: umbrellas
		});
		const text = runMessage(queueRun(t, 'webhook', '{"from":"school"}'));
		expect(text).toMatch(/^\[Automation "School mail" · its webhook was called · /);
		expect(text).toContain('<payload>\n{"from":"school"}\n</payload>');
		expect(text).toContain('reply with only NO_NOTIFICATION');
	});

	it.each([
		['NO_NOTIFICATION', true],
		['Nothing new.\n\n**NO_NOTIFICATION**', true],
		['NO_NOTIFICATION\nActually, it will rain.', false],
		['It will rain.', false]
	])('%j is silent: %s', (reply, silent) => {
		expect(isSilentReply(reply)).toBe(silent);
	});
});

describe('parseRunAt', () => {
	it('reads delays and local times', () => {
		const now = at(28, 6);
		expect(parseRunAt('30m', now)).toEqual(at(28, 6, 30));
		expect(parseRunAt('2 h', now)).toEqual(at(28, 8));
		expect(parseRunAt('1d', now)).toEqual(at(29, 6));
		expect(parseRunAt('2026-09-30 17:00', now)).toEqual(at(30, 17));
		expect(() => parseRunAt('tomorrow', now)).toThrow(`Can't read the time "tomorrow"`);
	});
});

it('puts webhook URLs under the public origin', () => {
	writeConfig({ authSecret: 'secret', origin: 'https://nolune.example.com/' });
	expect(webhookUrl('abc')).toBe('https://nolune.example.com/api/hooks/abc');
});
