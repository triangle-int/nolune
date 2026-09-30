import { dirname, join } from 'node:path';

/*
 * Where the relay keeps its files, shared by main.ts, which runs it, and admin.ts, which talks to
 * it: both read the same environment (in the container, compose.yaml's).
 */

/** The gateways file: RELAY_DATA, else data/gateways.json. */
export function dataFile(): string {
	return process.env.RELAY_DATA || 'data/gateways.json';
}

/** The operator's socket: RELAY_ADMIN_SOCKET, else admin.sock next to the gateways file. */
export function adminSocket(): string {
	return process.env.RELAY_ADMIN_SOCKET || join(dirname(dataFile()), 'admin.sock');
}
