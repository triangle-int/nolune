import http from 'node:http';
import { adminSocket } from './files.ts';
import type { GatewayInfo } from './relay.ts';

/*
 * The operator's commands, over the running relay's admin socket (files.ts). In the container:
 *
 *   docker compose exec relay node --no-warnings src/admin.ts list
 */

const USAGE = `usage: admin.ts <command>
  list                        every address: whether it's connected, its traffic this month
  show <name>                 one address
  block <name> [reason...]    take an address off the relay: its visitors see that it's blocked,
                              and its nolune can't connect. The reason is shown to its owner
  unblock <name>              let it back
  remove <name>               forget an address, so its name is free again`;

function request(method: string, path: string, body?: unknown): Promise<unknown> {
	return new Promise((resolve, reject) => {
		const req = http.request({ socketPath: adminSocket(), method, path }, async (res) => {
			let text = '';
			for await (const chunk of res) text += chunk;
			const answer = text ? (JSON.parse(text) as { error?: string }) : undefined;
			if ((res.statusCode ?? 500) >= 400) reject(new Error(answer?.error ?? `${res.statusCode}`));
			else resolve(answer);
		});
		req.on('error', (err) =>
			reject(
				new Error(`couldn't reach the relay at ${adminSocket()}: is it running? (${err.message})`)
			)
		);
		req.end(body === undefined ? undefined : JSON.stringify(body));
	});
}

function size(bytes: number): string {
	return bytes >= 1024 ** 3
		? `${(bytes / 1024 ** 3).toFixed(1)} GB`
		: `${Math.round(bytes / 1024 ** 2)} MB`;
}

function row(gateway: GatewayInfo): string[] {
	return [
		gateway.name,
		gateway.blocked ? `blocked: ${gateway.blocked}` : gateway.online ? 'online' : 'offline',
		size(gateway.trafficBytes),
		gateway.lastSeenAt?.slice(0, 16).replace('T', ' ') ?? 'never',
		gateway.createdAt.slice(0, 10)
	];
}

function table(rows: string[][]): string {
	const all = [['NAME', 'STATE', 'THIS MONTH', 'LAST SEEN (UTC)', 'CREATED'], ...rows];
	const widths = all[0].map((_, i) => Math.max(...all.map((r) => r[i].length)));
	return all
		.map((r) =>
			r
				.map((cell, i) => cell.padEnd(widths[i]))
				.join('  ')
				.trimEnd()
		)
		.join('\n');
}

async function main(args: string[]): Promise<void> {
	const [command, name, ...rest] = args;
	const path = `/gateways/${encodeURIComponent(name ?? '')}`;
	if (command === 'list') {
		const gateways = (await request('GET', '/gateways')) as GatewayInfo[];
		gateways.sort((a, b) => b.trafficBytes - a.trafficBytes || a.name.localeCompare(b.name));
		console.log(table(gateways.map(row)));
		console.log(`${gateways.length} addresses, ${gateways.filter((g) => g.online).length} online`);
		return;
	}
	if (!name || !['show', 'block', 'unblock', 'remove'].includes(command)) {
		console.error(USAGE);
		process.exitCode = 2;
		return;
	}
	if (command === 'remove') {
		await request('DELETE', path);
		console.log(`Removed ${name}: the name is free again.`);
		return;
	}
	const gateway = (await (command === 'show'
		? request('GET', path)
		: request('POST', `${path}/${command}`, { reason: rest.join(' ') }))) as GatewayInfo;
	console.log(table([row(gateway)]));
}

main(process.argv.slice(2)).catch((err: unknown) => {
	console.error((err as Error).message);
	process.exitCode = 1;
});
