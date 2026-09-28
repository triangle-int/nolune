import { chmodSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { cliCommand, paths } from './paths.ts';

/**
 * Writes ~/.nolune/bin/nolune, which the agent's commands have on PATH. It points at this
 * install's CLI and the node binary running right now.
 */
export function installCliShim(): string {
	const target = join(paths.bin, 'nolune');
	mkdirSync(paths.bin, { recursive: true });
	const command = cliCommand()
		.map((part) => JSON.stringify(part))
		.join(' ');
	writeFileSync(
		target,
		`#!/bin/sh\nexport NOLUNE_HOME="\${NOLUNE_HOME:-${paths.home}}"\nexec ${command} "$@"\n`
	);
	chmodSync(target, 0o755);
	return target;
}
