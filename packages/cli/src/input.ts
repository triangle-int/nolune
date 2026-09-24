import { createInterface } from 'node:readline/promises';

export async function readStdin(): Promise<string> {
	let data = '';
	for await (const chunk of process.stdin) data += chunk;
	return data.trim();
}

export async function ask(question: string, defaultValue = ''): Promise<string> {
	// Without a terminal (scripts, launchd) take the default rather than block; pass flags instead.
	if (!process.stdin.isTTY) return defaultValue;
	const rl = createInterface({ input: process.stdin, output: process.stdout });
	try {
		const suffix = defaultValue ? ` [${defaultValue}]` : '';
		return (await rl.question(`${question}${suffix}: `)).trim() || defaultValue;
	} finally {
		rl.close();
	}
}

/** Like ask(), but doesn't echo what's typed or pasted (API keys). */
export function askHidden(question: string): Promise<string> {
	if (!process.stdin.isTTY) return readStdin();
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
