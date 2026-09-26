import { setTimeout as sleep } from 'node:timers/promises';
import { parseArgs } from 'node:util';
import {
	EFFORTS,
	findSubagent,
	getConversation,
	listSubagents,
	requestSubagentStop,
	resolvePreset,
	runSubagent,
	steerSubagent,
	subagentLogPath,
	subagentResult,
	type Effort,
	type Subagent
} from '@btw/core';
import { readStdin } from './input.ts';

export const AGENT_HELP = `Subagents (in agent commands: agents that work on a task in the background, in a
conversation of their own that starts with only the task)
  btw agent run [<id>] --prompt "<task>" [--preset NAME] [--effort LEVEL]
                                             start a subagent, or give one that finished more work;
                                             prints its id and its log file (--prompt - reads stdin).
                                             It uses this chat's model and reasoning unless given:
                                             --preset takes a name from \`btw preset list\`, --effort
                                             one of ${EFFORTS.join(', ')}
  btw agent watch <id>                       wait until it's done and print its last message; run
                                             it with run_in_background to be told when it's done
  btw agent steer <id> --prompt "<message>"  message a subagent while it works
  btw agent stop <id>
  btw agent list`;

const WATCH_POLL_MS = 1000;

function parentId(): string {
	const id = process.env.BTW_CONVERSATION_ID;
	if (!id) {
		throw new Error(
			"`btw agent` only works in the agent's commands: subagents belong to the conversation that starts them."
		);
	}
	return id;
}

async function promptFrom(value: string | undefined, usage: string): Promise<string> {
	const text = value === '-' ? await readStdin() : value?.trim();
	if (!text) throw new Error(`usage: ${usage}`);
	return text;
}

function name(positionals: string[], usage: string): string {
	const given = positionals[0];
	if (!given) throw new Error(`usage: ${usage}`);
	return given;
}

function subagent(parent: string, id: string): Subagent {
	const found = findSubagent(parent, id);
	if (!found) {
		throw new Error(`this conversation has no subagent "${id}". \`btw agent list\` shows them.`);
	}
	return found;
}

export async function agentCommand(action: string | undefined, args: string[]): Promise<void> {
	const { values, positionals } = parseArgs({
		args,
		allowPositionals: true,
		options: {
			prompt: { type: 'string' },
			preset: { type: 'string' },
			effort: { type: 'string' }
		}
	});
	switch (action) {
		case 'run': {
			const usage =
				'btw agent run [<id>] --prompt "<task>" [--preset NAME] [--effort LEVEL] (--prompt - reads stdin)';
			const prompt = await promptFrom(values.prompt, usage);
			if (values.effort !== undefined && !EFFORTS.includes(values.effort as Effort)) {
				throw new Error(`--effort must be one of ${EFFORTS.join(', ')}.`);
			}
			const {
				subagent: s,
				conversation,
				created
			} = runSubagent({
				parentId: parentId(),
				name: positionals[0],
				prompt,
				// Names as `btw preset list` shows them; an unknown one says to look there.
				presetId: values.preset ? resolvePreset(values.preset)?.id : undefined,
				effort: values.effort as Effort | undefined
			});
			const model = `${conversation.presetName}, reasoning ${conversation.effort}`;
			console.log(
				created ? `Started subagent ${s.name} (${model}).` : `Gave ${s.name} more work (${model}).`
			);
			const log = subagentLogPath(s);
			if (log) {
				console.log(
					`Log: ${log} (what it says and runs as it works, without its reasoning; check it with tail)`
				);
			}
			console.log(
				`To be told when it's done, run \`btw agent watch ${s.name}\` with run_in_background: its last message becomes that command's output. Steer it while it works with \`btw agent steer ${s.name} --prompt "..."\`.`
			);
			return;
		}

		case 'watch': {
			const parent = parentId();
			const id = name(positionals, 'btw agent watch <id>');
			for (;;) {
				const result = subagentResult(subagent(parent, id));
				if (result) {
					console.log(result.text);
					if (!result.ok) process.exitCode = 1;
					return;
				}
				await sleep(WATCH_POLL_MS);
			}
		}

		case 'steer': {
			const usage = 'btw agent steer <id> --prompt "<message>" (or --prompt - to read stdin)';
			const id = name(positionals, usage);
			const text = await promptFrom(values.prompt, usage);
			const s = steerSubagent({ parentId: parentId(), name: id, text });
			console.log(`Sent ${s.name} your message; it reads it at its next step.`);
			return;
		}

		case 'stop': {
			const s = requestSubagentStop({
				parentId: parentId(),
				name: name(positionals, 'btw agent stop <id>')
			});
			console.log(`Stopping ${s.name}.`);
			return;
		}

		case 'list':
		case undefined: {
			const all = listSubagents(parentId());
			if (!all.length) {
				console.log('This conversation has no subagents.');
				return;
			}
			for (const s of all) {
				const conv = getConversation(s.conversationId);
				const status = s.error && s.status !== 'done' ? `${s.status} (${s.error})` : s.status;
				console.log(`${s.name}  ${status}`);
				if (conv?.title) console.log(`  ${conv.title}`);
				if (conv) console.log(`  ${conv.presetName}, reasoning ${conv.effort}`);
				const log = subagentLogPath(s);
				if (log) console.log(`  log: ${log}`);
			}
			return;
		}

		default:
			throw new Error('usage: btw agent run|watch|steer|stop|list. See `btw help`.');
	}
}
