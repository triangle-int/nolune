import { createRelay } from './relay.ts';
import { GatewayStore } from './store.ts';

/*
 * Runs the relay (see README.md next to this folder):
 *
 *   RELAY_DOMAIN=nolune.dev RELAY_HOST=relay.nolune.dev node src/main.ts
 *
 * RELAY_DOMAIN   gateways get <name>.<domain> (required)
 * RELAY_HOST     where gateways register and connect; the domain itself by default
 * RELAY_DATA     the gateways file; data/gateways.json by default
 * RELAY_TRUST_PROXY=1  take the client's address from X-Forwarded-For, as set by Caddy in front
 * RELAY_SCHEME=http    for trying it without TLS; addresses are https otherwise
 * HOST, PORT     where it listens; 0.0.0.0:8080 by default
 */

const domain = process.env.RELAY_DOMAIN;
if (!domain) {
	console.error('Set RELAY_DOMAIN: gateways get addresses under it, like <name>.nolune.dev.');
	process.exit(1);
}

const relay = createRelay({
	domain,
	host: process.env.RELAY_HOST || undefined,
	scheme: process.env.RELAY_SCHEME === 'http' ? 'http' : 'https',
	store: new GatewayStore(process.env.RELAY_DATA || 'data/gateways.json'),
	trustProxy: process.env.RELAY_TRUST_PROXY === '1',
	log: (message) => console.log(`[relay] ${message}`)
});

const host = process.env.HOST || '0.0.0.0';
const port = Number(process.env.PORT || 8080);
relay.server.listen(port, host, () => {
	console.log(`[relay] listening on ${host}:${port} for ${process.env.RELAY_HOST || domain}`);
});

for (const signal of ['SIGINT', 'SIGTERM'] as const) {
	process.once(signal, () => void relay.close().then(() => process.exit(0)));
}
