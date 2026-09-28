import { parseArgs } from 'node:util';
import {
	EFFORTS,
	createTrigger,
	deleteTrigger,
	describeWhen,
	findTrigger,
	formatLocalTime,
	getPreset,
	getProfile,
	getTrigger,
	isFinished,
	listProfiles,
	listRuns,
	listTriggers,
	parseRunAt,
	queueRun,
	queueWake,
	resolvePreset,
	setTriggerEnabled,
	updateTrigger,
	webhookUrl,
	type Effort,
	type Profile,
	type Trigger,
	type TriggerWhat,
	type TriggerWhen
} from '@nolune/core';
import type { Io } from './io.ts';
import { profileFor } from './profile.ts';

export const TRIGGER_HELP = `Automations (results show up as notifications in the web UI)
  nolune trigger add <name> WHEN WHAT [--summary S] [--icon I] [--preset NAME] [--effort LEVEL]
                     [--profile SLUG]
      WHEN: --cron "<min hour day month weekday>" (local time) | --at "YYYY-MM-DD HH:MM"
            | --in 30m|2h|1d | --webhook
      WHAT: --prompt "<what the agent should do>" | --script "<shell command, no model>"
      --summary: one plain sentence the family sees on the Automations page
      --icon: a Lucide icon name for it, like umbrella
  nolune trigger list [--profile SLUG]
  nolune trigger show|run|pause|resume|rm <name|id>
  nolune trigger edit <name|id> [--name N] [--summary S] [--icon I] [WHEN] [WHAT] [--preset NAME]
                      [--effort LEVEL]
  nolune wake <message> [--title T] [--profile SLUG]
      start a background agent run now; trigger scripts call this (\`nolune wake -\` reads stdin)`;

const OPTIONS = {
	cron: { type: 'string' },
	at: { type: 'string' },
	in: { type: 'string' },
	webhook: { type: 'boolean' },
	prompt: { type: 'string' },
	script: { type: 'string' },
	name: { type: 'string' },
	summary: { type: 'string' },
	icon: { type: 'string' },
	title: { type: 'string' },
	preset: { type: 'string' },
	effort: { type: 'string' },
	profile: { type: 'string' }
} as const;

type Values = ReturnType<typeof parse>['values'];

function parse(args: string[]) {
	return parseArgs({ args, allowPositionals: true, options: OPTIONS });
}

function optionalProfile(io: Io, flag: string | undefined): Profile | undefined {
	return flag || io.env.NOLUNE_PROFILE ? profileFor(io, flag) : undefined;
}

function whenFrom(values: Values): TriggerWhen | undefined {
	const given = [values.cron, values.at, values.in, values.webhook].filter((v) => v !== undefined);
	if (given.length > 1) throw new Error('give only one of --cron, --at, --in and --webhook');
	if (values.cron !== undefined) return { kind: 'cron', cron: values.cron };
	if (values.at !== undefined) return { kind: 'once', runAt: parseRunAt(values.at) };
	if (values.in !== undefined) return { kind: 'once', runAt: parseRunAt(values.in) };
	if (values.webhook) return { kind: 'webhook' };
	return undefined;
}

function whatFrom(values: Values): TriggerWhat | undefined {
	if (values.prompt !== undefined && values.script !== undefined) {
		throw new Error('give either --prompt or --script, not both');
	}
	if (values.prompt !== undefined) return { action: 'agent', prompt: values.prompt };
	if (values.script !== undefined) return { action: 'script', command: values.script };
	return undefined;
}

function effortFrom(values: Values): Effort | undefined {
	if (values.effort === undefined) return undefined;
	if (!EFFORTS.includes(values.effort as Effort)) {
		throw new Error(`--effort must be one of ${EFFORTS.join(', ')}`);
	}
	return values.effort as Effort;
}

function shortId(t: Trigger): string {
	return t.id.slice(0, 8);
}

function status(t: Trigger): string {
	if (isFinished(t)) return 'done';
	if (!t.enabled) return 'paused';
	if (t.nextRunAt) return `next ${formatLocalTime(t.nextRunAt)}`;
	return 'waiting for its webhook';
}

function printWebhook(io: Io, t: Trigger): void {
	if (!t.webhookToken) return;
	const url = webhookUrl(t.webhookToken);
	io.log(`Webhook URL (keep it secret: anyone with it can start a run):
  ${url}
Send JSON; form-encoded and text/plain bodies are refused (cross-site protection), e.g.:
  curl -X POST -H 'content-type: application/json' -d '{"text":"hello"}' '${url}'`);
}

function describeWhat(t: Trigger): string {
	return t.action === 'agent' ? 'wakes the agent with its prompt' : `runs \`${t.command}\``;
}

export function triggerCommand(io: Io, action: string | undefined, args: string[]): void {
	const { values, positionals } = parse(args);
	const ref = () => {
		if (!positionals[0]) throw new Error('missing <name|id>. See `nolune trigger list`.');
		return findTrigger(positionals[0], optionalProfile(io, values.profile)?.id);
	};

	switch (action) {
		case undefined:
		case 'help':
			io.log(TRIGGER_HELP);
			return;

		case 'add': {
			const name = positionals[0];
			if (!name) throw new Error('missing <name>. See `nolune help`.');
			const when = whenFrom(values);
			if (!when) throw new Error('say when: --cron, --at, --in or --webhook');
			const what = whatFrom(values);
			if (!what) throw new Error('say what: --prompt or --script');
			const profile = profileFor(io, values.profile);
			const preset = resolvePreset(values.preset, io.env.NOLUNE_CONVERSATION_ID);
			const t = createTrigger({
				profileId: profile.id,
				name,
				summary: values.summary,
				icon: values.icon,
				when,
				what,
				presetId: preset?.id ?? null,
				effort: effortFrom(values)
			});
			io.log(
				`Added "${t.name}" (${shortId(t)}) to ${profile.slug}: ${describeWhen(t)}, ${describeWhat(t)}.`
			);
			if (t.nextRunAt) io.log(`Next run: ${formatLocalTime(t.nextRunAt)}`);
			printWebhook(io, t);
			return;
		}

		case 'list': {
			const profile = optionalProfile(io, values.profile);
			const slugs = new Map(listProfiles().map((p) => [p.id, p.slug]));
			const triggers = listTriggers(profile?.id);
			if (!triggers.length) io.log('No triggers.');
			for (const t of triggers) {
				const where = profile ? '' : `${slugs.get(t.profileId)}\t`;
				io.log(
					`${shortId(t)}\t${where}${t.name}\t${describeWhen(t)}\t${t.action === 'agent' ? 'prompt' : 'script'}\t${status(t)}`
				);
			}
			return;
		}

		case 'show': {
			const t = ref();
			const preset = t.presetId ? getPreset(t.presetId) : undefined;
			io.log(`${t.name} (${t.id})
  summary  ${t.summary ?? '(none: the Automations page shows only the name)'}
  icon     ${t.icon ?? '(none)'}
  profile  ${getProfile(t.profileId)?.slug ?? '?'}
  when     ${describeWhen(t)} (${status(t)})
  ${t.action === 'agent' ? `prompt   ${t.prompt}` : `script   ${t.command}`}
  model    ${preset?.name ?? 'default preset'}, reasoning ${t.effort}`);
			printWebhook(io, t);
			const runs = listRuns(t.id, 10);
			io.log(runs.length ? 'Recent runs:' : 'No runs yet.');
			for (const run of runs) {
				const firstLine =
					run.status === 'failed' && run.output ? `: ${run.output.split('\n')[0]}` : '';
				io.log(
					`  ${formatLocalTime(run.createdAt)}\t${run.action}\t${run.source}\t${run.status}${firstLine}`
				);
			}
			const lastScript = runs.find((r) => r.action === 'script' && r.output);
			if (lastScript) io.log(`Output of the last script run:\n${lastScript.output}`);
			return;
		}

		case 'run': {
			const t = ref();
			queueRun(t, 'manual');
			io.log(`Queued "${t.name}". The gateway starts it within a few seconds.`);
			return;
		}

		case 'pause':
		case 'resume': {
			const t = setTriggerEnabled(ref().id, action === 'resume');
			io.log(`"${t.name}" is ${action === 'resume' ? `on (${status(t)})` : 'paused'}.`);
			return;
		}

		case 'rm': {
			const t = ref();
			deleteTrigger(t.id);
			io.log(`Removed "${t.name}".`);
			return;
		}

		case 'edit': {
			const current = ref();
			const preset = values.preset ? resolvePreset(values.preset) : undefined;
			const t = updateTrigger(current.id, {
				name: values.name,
				summary: values.summary,
				icon: values.icon,
				when: whenFrom(values),
				what: whatFrom(values),
				presetId: preset?.id,
				effort: effortFrom(values)
			});
			io.log(`Updated "${t.name}": ${describeWhen(t)}, ${describeWhat(t)} (${status(t)}).`);
			if (t.webhookToken && !current.webhookToken) printWebhook(io, t);
			return;
		}

		default:
			throw new Error('usage: nolune trigger help|add|list|show|run|pause|resume|rm|edit');
	}
}

/** Queues a background agent run. Inside a trigger's script, the run belongs to that trigger. */
export async function wakeCommand(io: Io, args: string[]): Promise<void> {
	const { values, positionals } = parse(args);
	const given = positionals.join(' ').trim();
	// From a terminal, nothing was piped in: `-` is then an empty message.
	const text = given === '-' ? (io.stdinIsTTY ? '' : await io.readStdin()).trim() : given;
	if (!text)
		throw new Error('usage: nolune wake <what happened and what to do> (or - to read stdin)');
	const t = io.env.NOLUNE_TRIGGER_ID ? getTrigger(io.env.NOLUNE_TRIGGER_ID) : undefined;
	const profileId = t?.profileId ?? profileFor(io, values.profile).id;
	const preset =
		values.preset || !t ? resolvePreset(values.preset, io.env.NOLUNE_CONVERSATION_ID) : undefined;
	const run = queueWake({
		profileId,
		triggerId: t?.id,
		text,
		title: values.title,
		presetId: preset?.id,
		effort: effortFrom(values)
	});
	io.log(`Woke nolune ("${run.title}"). Its reply shows up as a notification.`);
}
