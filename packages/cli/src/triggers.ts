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
	getProfileBySlug,
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
} from '@btw/core';

export const TRIGGER_HELP = `Automations (results show up as notifications in the web UI)
  btw trigger add <name> WHEN WHAT [--preset NAME] [--effort LEVEL] [--profile SLUG]
      WHEN: --cron "<min hour day month weekday>" (local time) | --at "YYYY-MM-DD HH:MM"
            | --in 30m|2h|1d | --webhook
      WHAT: --prompt "<what the agent should do>" | --script "<shell command, no model>"
  btw trigger list [--profile SLUG]
  btw trigger show|run|pause|resume|rm <name|id>
  btw trigger edit <name|id> [--name N] [WHEN] [WHAT] [--preset NAME] [--effort LEVEL]
  btw wake <message> [--title T] [--profile SLUG]
      start a background agent run now; trigger scripts call this (\`btw wake -\` reads stdin)`;

const OPTIONS = {
	cron: { type: 'string' },
	at: { type: 'string' },
	in: { type: 'string' },
	webhook: { type: 'boolean' },
	prompt: { type: 'string' },
	script: { type: 'string' },
	name: { type: 'string' },
	title: { type: 'string' },
	preset: { type: 'string' },
	effort: { type: 'string' },
	profile: { type: 'string' }
} as const;

type Values = ReturnType<typeof parse>['values'];

function parse(args: string[]) {
	return parseArgs({ args, allowPositionals: true, options: OPTIONS });
}

function profileFor(flag: string | undefined): Profile {
	const slug = flag || process.env.BTW_PROFILE;
	if (!slug) throw new Error('which profile? Pass --profile <slug>. See `btw profile list`.');
	const found = getProfileBySlug(slug);
	if (!found) throw new Error(`no profile with slug "${slug}". See \`btw profile list\`.`);
	return found;
}

function optionalProfile(flag: string | undefined): Profile | undefined {
	return flag || process.env.BTW_PROFILE ? profileFor(flag) : undefined;
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

function printWebhook(t: Trigger): void {
	if (!t.webhookToken) return;
	const url = webhookUrl(t.webhookToken);
	console.log(`Webhook URL (keep it secret: anyone with it can start a run):
  ${url}
Send JSON; form-encoded and text/plain bodies are refused (cross-site protection), e.g.:
  curl -X POST -H 'content-type: application/json' -d '{"text":"hello"}' '${url}'`);
}

function describeWhat(t: Trigger): string {
	return t.action === 'agent' ? 'wakes the agent with its prompt' : `runs \`${t.command}\``;
}

export function triggerCommand(action: string | undefined, args: string[]): void {
	const { values, positionals } = parse(args);
	const ref = () => {
		if (!positionals[0]) throw new Error('missing <name|id>. See `btw trigger list`.');
		return findTrigger(positionals[0], optionalProfile(values.profile)?.id);
	};

	switch (action) {
		case undefined:
		case 'help':
			console.log(TRIGGER_HELP);
			return;

		case 'add': {
			const name = positionals[0];
			if (!name) throw new Error('missing <name>. See `btw help`.');
			const when = whenFrom(values);
			if (!when) throw new Error('say when: --cron, --at, --in or --webhook');
			const what = whatFrom(values);
			if (!what) throw new Error('say what: --prompt or --script');
			const profile = profileFor(values.profile);
			const preset = resolvePreset(values.preset, process.env.BTW_CONVERSATION_ID);
			const t = createTrigger({
				profileId: profile.id,
				name,
				when,
				what,
				presetId: preset?.id ?? null,
				effort: effortFrom(values)
			});
			console.log(
				`Added "${t.name}" (${shortId(t)}) to ${profile.slug}: ${describeWhen(t)}, ${describeWhat(t)}.`
			);
			if (t.nextRunAt) console.log(`Next run: ${formatLocalTime(t.nextRunAt)}`);
			printWebhook(t);
			return;
		}

		case 'list': {
			const profile = optionalProfile(values.profile);
			const slugs = new Map(listProfiles().map((p) => [p.id, p.slug]));
			const triggers = listTriggers(profile?.id);
			if (!triggers.length) console.log('No triggers.');
			for (const t of triggers) {
				const where = profile ? '' : `${slugs.get(t.profileId)}\t`;
				console.log(
					`${shortId(t)}\t${where}${t.name}\t${describeWhen(t)}\t${t.action === 'agent' ? 'prompt' : 'script'}\t${status(t)}`
				);
			}
			return;
		}

		case 'show': {
			const t = ref();
			const preset = t.presetId ? getPreset(t.presetId) : undefined;
			console.log(`${t.name} (${t.id})
  profile  ${getProfile(t.profileId)?.slug ?? '?'}
  when     ${describeWhen(t)} (${status(t)})
  ${t.action === 'agent' ? `prompt   ${t.prompt}` : `script   ${t.command}`}
  model    ${preset?.name ?? 'default preset'}, reasoning ${t.effort}`);
			printWebhook(t);
			const runs = listRuns(t.id, 10);
			console.log(runs.length ? 'Recent runs:' : 'No runs yet.');
			for (const run of runs) {
				const firstLine =
					run.status === 'failed' && run.output ? `: ${run.output.split('\n')[0]}` : '';
				console.log(
					`  ${formatLocalTime(run.createdAt)}\t${run.action}\t${run.source}\t${run.status}${firstLine}`
				);
			}
			const lastScript = runs.find((r) => r.action === 'script' && r.output);
			if (lastScript) console.log(`Output of the last script run:\n${lastScript.output}`);
			return;
		}

		case 'run': {
			const t = ref();
			queueRun(t, 'manual');
			console.log(`Queued "${t.name}". The gateway starts it within a few seconds.`);
			return;
		}

		case 'pause':
		case 'resume': {
			const t = setTriggerEnabled(ref().id, action === 'resume');
			console.log(`"${t.name}" is ${action === 'resume' ? `on (${status(t)})` : 'paused'}.`);
			return;
		}

		case 'rm': {
			const t = ref();
			deleteTrigger(t.id);
			console.log(`Removed "${t.name}".`);
			return;
		}

		case 'edit': {
			const current = ref();
			const preset = values.preset ? resolvePreset(values.preset) : undefined;
			const t = updateTrigger(current.id, {
				name: values.name,
				when: whenFrom(values),
				what: whatFrom(values),
				presetId: preset?.id,
				effort: effortFrom(values)
			});
			console.log(`Updated "${t.name}": ${describeWhen(t)}, ${describeWhat(t)} (${status(t)}).`);
			if (t.webhookToken && !current.webhookToken) printWebhook(t);
			return;
		}

		default:
			throw new Error('usage: btw trigger help|add|list|show|run|pause|resume|rm|edit');
	}
}

async function readStdin(): Promise<string> {
	if (process.stdin.isTTY) return '';
	const chunks: Buffer[] = [];
	for await (const chunk of process.stdin) chunks.push(chunk as Buffer);
	return Buffer.concat(chunks).toString('utf8');
}

/** Queues a background agent run. Inside a trigger's script, the run belongs to that trigger. */
export async function wakeCommand(args: string[]): Promise<void> {
	const { values, positionals } = parse(args);
	const given = positionals.join(' ').trim();
	const text = given === '-' ? (await readStdin()).trim() : given;
	if (!text) throw new Error('usage: btw wake <what happened and what to do> (or - to read stdin)');
	const t = process.env.BTW_TRIGGER_ID ? getTrigger(process.env.BTW_TRIGGER_ID) : undefined;
	const profileId = t?.profileId ?? profileFor(values.profile).id;
	const preset =
		values.preset || !t ? resolvePreset(values.preset, process.env.BTW_CONVERSATION_ID) : undefined;
	const run = queueWake({
		profileId,
		triggerId: t?.id,
		text,
		title: values.title,
		presetId: preset?.id,
		effort: effortFrom(values)
	});
	console.log(`Woke btw ("${run.title}"). Its reply shows up as a notification.`);
}
