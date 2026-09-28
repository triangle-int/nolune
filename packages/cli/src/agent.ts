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
} from '@nolune/core';
import type { Io } from './io.ts';

export const AGENT_HELP = `Subagents (in agent commands: agents that work on a task in the background, in a
conversation of their own that starts with only the task)
  nolune agent run [<id>] --prompt "<task>" [--preset NAME] [--effort LEVEL]
                                             start a subagent, or give one that finished more work;
                                             prints its id and its log file (--prompt - reads stdin).
                                             It uses this chat's model and reasoning unless given:
                                             --preset takes a name from \`nolune preset list\`, --effort
                                             one of ${EFFORTS.join(', ')}
  nolune agent watch <id>                    wait until it's done and print its last message; run
                                             it with run_in_background to be told when it's done
  nolune agent steer <id> --prompt "<message>"
                                             message a subagent while it works
  nolune agent stop <id>
  nolune agent list`;

const WATCH_POLL_MS = 1000;

function parentId(io: Io): string {
	const id = io.env.NOLUNE_CONVERSATION_ID;
	if (!id) {
		throw new Error(
			"`nolune agent` only works in the agent's commands: subagents belong to the conversation that starts them."
		);
	}
	return id;
}

async function promptFrom(io: Io, value: string | undefined, usage: string): Promise<string> {
	const text = (value === '-' ? await io.readStdin() : value)?.trim();
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
		throw new Error(`this conversation has no subagent "${id}". \`nolune agent list\` shows them.`);
	}
	return found;
}

/** The exit code: `watch` fails with 1 when the subagent failed or was stopped. */
export async function agentCommand(
	io: Io,
	action: string | undefined,
	args: string[]
): Promise<number | void> {
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
				'nolune agent run [<id>] --prompt "<task>" [--preset NAME] [--effort LEVEL] (--prompt - reads stdin)';
			const prompt = await promptFrom(io, values.prompt, usage);
			if (values.effort !== undefined && !EFFORTS.includes(values.effort as Effort)) {
				throw new Error(`--effort must be one of ${EFFORTS.join(', ')}.`);
			}
			const {
				subagent: s,
				conversation,
				created
			} = runSubagent({
				parentId: parentId(io),
				name: positionals[0],
				prompt,
				// Names as `nolune preset list` shows them; an unknown one says to look there.
				presetId: values.preset ? resolvePreset(values.preset)?.id : undefined,
				effort: values.effort as Effort | undefined
			});
			const model = `${conversation.presetName}, reasoning ${conversation.effort}`;
			io.log(
				created ? `Started subagent ${s.name} (${model}).` : `Gave ${s.name} more work (${model}).`
			);
			const log = subagentLogPath(s);
			if (log) {
				io.log(
					`Log: ${log} (what it says and runs as it works, without its reasoning; check it with tail)`
				);
			}
			io.log(
				`To be told when it's done, run \`nolune agent watch ${s.name}\` with run_in_background: its last message becomes that command's output. Steer it while it works with \`nolune agent steer ${s.name} --prompt "..."\`.`
			);
			return;
		}

		case 'watch': {
			const parent = parentId(io);
			const id = name(positionals, 'nolune agent watch <id>');
			for (;;) {
				const result = subagentResult(subagent(parent, id));
				if (result) {
					io.log(result.text);
					return result.ok ? 0 : 1;
				}
				await sleep(WATCH_POLL_MS, undefined, { signal: io.signal });
			}
		}

		case 'steer': {
			const usage = 'nolune agent steer <id> --prompt "<message>" (or --prompt - to read stdin)';
			const id = name(positionals, usage);
			const text = await promptFrom(io, values.prompt, usage);
			const s = steerSubagent({ parentId: parentId(io), name: id, text });
			io.log(`Sent ${s.name} your message; it reads it at its next step.`);
			return;
		}

		case 'stop': {
			const s = requestSubagentStop({
				parentId: parentId(io),
				name: name(positionals, 'nolune agent stop <id>')
			});
			io.log(`Stopping ${s.name}.`);
			return;
		}

		case 'list':
		case undefined: {
			const all = listSubagents(parentId(io));
			if (!all.length) {
				io.log('This conversation has no subagents.');
				return;
			}
			for (const s of all) {
				const conv = getConversation(s.conversationId);
				const status = s.error && s.status !== 'done' ? `${s.status} (${s.error})` : s.status;
				io.log(`${s.name}  ${status}`);
				if (conv?.title) io.log(`  ${conv.title}`);
				if (conv) io.log(`  ${conv.presetName}, reasoning ${conv.effort}`);
				const log = subagentLogPath(s);
				if (log) io.log(`  log: ${log}`);
			}
			return;
		}

		default:
			throw new Error('usage: nolune agent run|watch|steer|stop|list. See `nolune help`.');
	}
}
