#!/usr/bin/env node
import { paths } from '@btw/core/paths';
import { runInGateway } from './client.ts';

/*
 * `btw`. The agent runs it constantly, so it first asks the gateway to run the command, where
 * btw is already loaded; only when the gateway doesn't does this process load btw and run it.
 * A person at a terminal always gets a process of their own: a command may ask them something.
 */

async function readStdin(): Promise<string> {
	let data = '';
	for await (const chunk of process.stdin) data += chunk;
	return data;
}

const argv = process.argv.slice(2);
const served = process.stdin.isTTY
	? null
	: await runInGateway(argv, {
			socketPath: paths.cliSocket,
			cwd: process.cwd(),
			env: process.env,
			stdio: {
				stdout: (text) => process.stdout.write(text),
				stderr: (text) => process.stderr.write(text),
				readStdin
			}
		});

if (served !== null) process.exitCode = served;
else {
	const [{ runCli }, { processIo }] = await Promise.all([import('./run.ts'), import('./io.ts')]);
	process.exitCode = await runCli(argv, processIo());
}
