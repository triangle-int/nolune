import { createInterface } from 'node:readline/promises';
import type { Io } from './io.ts';

/*
 * Questions for the person at the terminal. Only a process's own stdin can be a terminal, so the
 * interactive side talks to `process` directly; without one, the answer comes from the command's
 * stdin, or the default.
 */

export async function ask(io: Io, question: string, defaultValue = ''): Promise<string> {
	// Without a terminal (scripts, launchd) take the default rather than block; pass flags instead.
	if (!io.stdinIsTTY) return defaultValue;
	const rl = createInterface({ input: process.stdin, output: process.stdout });
	try {
		const suffix = defaultValue ? ` [${defaultValue}]` : '';
		return (await rl.question(`${question}${suffix}: `)).trim() || defaultValue;
	} finally {
		rl.close();
	}
}

/** Like ask(), but doesn't echo what's typed or pasted (API keys). */
export async function askHidden(io: Io, question: string): Promise<string> {
	if (!io.stdinIsTTY) return (await io.readStdin()).trim();
	const stdin = process.stdin;
	return new Promise((resolve, reject) => {
		process.stdout.write(`${question}: `);
		let value = '';
		const cleanup = () => {
			stdin.off('data', onData);
			stdin.setRawMode(false);
			stdin.pause();
			process.stdout.write('\n');
		};
		const onData = (chunk: string) => {
			for (const ch of chunk) {
				if (ch === '\r' || ch === '\n') {
					cleanup();
					resolve(value.trim());
					return;
				}
				if (ch === '\u0003') {
					cleanup();
					reject(new Error('Cancelled'));
					return;
				}
				if (ch === '\u007f' || ch === '\b') value = value.slice(0, -1);
				else value += ch;
			}
		};
		stdin.setRawMode(true);
		stdin.setEncoding('utf8');
		stdin.resume();
		stdin.on('data', onData);
	});
}
