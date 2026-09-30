import { chmodSync, mkdirSync, rmSync } from 'node:fs';
import { dirname } from 'node:path';
import { adminSocket, dataFile } from './files.ts';
import { createRelay } from './relay.ts';
import { GatewayStore } from './store.ts';

/*
 * Runs the relay (see README.md next to this folder):
 *
 *   RELAY_DOMAIN=nolune.family RELAY_HOST=relay.nolune.dev node src/main.ts
 *
 * RELAY_DOMAIN   gateways get <name>.<domain> (required)
 * RELAY_HOST     where gateways register and connect; the domain itself by default
 * RELAY_SITE_URL where the relay's host and the bare domain send browsers; https://nolune.dev by
 *                default, empty for a line of text instead
 * RELAY_DATA     the gateways file; data/gateways.json by default
 * RELAY_MONTHLY_GB     traffic each address may pass in a month; 30 by default, 0 for no limit
 * RELAY_MAX_PER_NETWORK  addresses one network may have; 10 by default, 0 for no limit
 * RELAY_FORGET_AFTER_DAYS  days a gateway may stay away before its name is free again; 90 by
 *                default, 0 to keep every name
 * RELAY_ADMIN_SOCKET   the operator's socket (admin.ts); admin.sock next to the gateways file
 * RELAY_TRUST_PROXY=1  take the client's address from X-Forwarded-For, as set by Caddy in front
 * RELAY_SCHEME=http    for trying it without TLS; addresses are https otherwise
 * HOST, PORT     where it listens; 0.0.0.0:8080 by default
 */

const domain = process.env.RELAY_DOMAIN;
if (!domain) {
	console.error('Set RELAY_DOMAIN: gateways get addresses under it, like <name>.nolune.family.');
	process.exit(1);
}

/** A whole number from the environment, or the default when it's unset or not one. */
function setting(name: string, fallback: number): number {
	const value = Number(process.env[name] || NaN);
	return Number.isFinite(value) && value >= 0 ? value : fallback;
}

const relay = createRelay({
	domain,
	host: process.env.RELAY_HOST || undefined,
	site: process.env.RELAY_SITE_URL ?? 'https://nolune.dev',
	scheme: process.env.RELAY_SCHEME === 'http' ? 'http' : 'https',
	store: new GatewayStore(dataFile()),
	trustProxy: process.env.RELAY_TRUST_PROXY === '1',
	monthlyTrafficBytes: Math.round(setting('RELAY_MONTHLY_GB', 30) * 1024 ** 3),
	maxGatewaysPerNetwork: setting('RELAY_MAX_PER_NETWORK', 10),
	forgetAfterMs: setting('RELAY_FORGET_AFTER_DAYS', 90) * 24 * 60 * 60 * 1000,
	log: (message) => console.log(`[relay] ${message}`)
});

const host = process.env.HOST || '0.0.0.0';
const port = Number(process.env.PORT || 8080);
relay.server.listen(port, host, () => {
	console.log(`[relay] listening on ${host}:${port} for ${process.env.RELAY_HOST || domain}`);
});

// Only whoever can open the socket file (the relay's own user) can administer it.
const socket = adminSocket();
mkdirSync(dirname(socket), { recursive: true });
// Left by a relay that didn't shut down cleanly.
rmSync(socket, { force: true });
relay.admin.listen(socket, () => chmodSync(socket, 0o600));

for (const signal of ['SIGINT', 'SIGTERM'] as const) {
	process.once(signal, () => void relay.close().then(() => process.exit(0)));
}
