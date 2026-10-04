import { parseArgs } from 'node:util';
import { describeBackground, requestBackgroundStop } from '@nolune/core';
import type { Io } from './io.ts';

export const BACKGROUND_HELP = `Background work (in agent commands: what the conversation's agent has going while it does other
things)
  nolune background                          list it: commands started with run_in_background, by
                                             their process group, and working subagents, by their
                                             id, with how long each has run
  nolune background stop <id>... | --all     stop them within seconds; nothing they would have
                                             handed over arrives`;

function conversationId(io: Io): string {
	const id = io.env.NOLUNE_CONVERSATION_ID;
	if (!id) {
		throw new Error(
			"`nolune background` only works in the agent's commands: it's about the conversation that runs it."
		);
	}
	return id;
}

export async function backgroundCommand(
	io: Io,
	action: string | undefined,
	args: string[]
): Promise<void> {
	const { values, positionals } = parseArgs({
		args,
		allowPositionals: true,
		options: { all: { type: 'boolean' } }
	});
	switch (action) {
		case 'list':
		case undefined: {
			const lines = describeBackground(conversationId(io), { withLogs: true });
			if (!lines.length) {
				io.log("Nothing runs in this conversation's background.");
				return;
			}
			for (const line of lines) io.log(line);
			io.log(
				'\nStop one with `nolune background stop <id>`, or all of it with `nolune background stop --all`.'
			);
			return;
		}

		case 'stop': {
			if (!values.all && !positionals.length) {
				throw new Error('usage: nolune background stop <id>... | --all');
			}
			const { commands, subagents } = requestBackgroundStop(
				conversationId(io),
				values.all ? 'all' : positionals,
				'the agent'
			);
			const stopping = [
				...commands.map((c) => `${c.pid}${c.summary ? ` (${c.summary})` : ''}`),
				...subagents.map((s) => s.name)
			];
			if (!stopping.length) {
				io.log("Nothing runs in this conversation's background.");
				return;
			}
			io.log(
				`Stopping ${stopping.join(', ')}. Nothing ${stopping.length > 1 ? 'they' : 'it'} would have handed over arrives.`
			);
			return;
		}

		default:
			throw new Error('usage: nolune background [list] | stop <id>... | --all. See `nolune help`.');
	}
}
