import { createHash, randomBytes } from 'node:crypto';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import { fakeServer } from './mcp-server.ts';

/**
 * The fake MCP server (fakeServer) behind a sign-in, as MCP has it, with its own authorization
 * server at the same address: it says where that is (RFC 9728), lets clients register (RFC 7591)
 * unless `registration` is off, and trades codes (PKCE) and refresh tokens for tokens. `approve`
 * stands in for someone signing in in a browser; its sign-in page says yes at once.
 */
export interface OAuthServer {
	/** The MCP server's address. */
	url: string;
	origin: string;
	/** How many clients registered themselves. */
	registered: number;
	/** The scope each token request asked for, in order. */
	scopes: (string | null)[];
	/** Signs in at the page `authorizationUrl` is: what the browser brings back to the client. */
	approve(authorizationUrl: URL): { code: string; state: string; redirectUri: string };
	/** The access tokens given so far stop working, as when they expire. */
	expireAccessTokens(): void;
	/** So do the refresh tokens: someone has to sign in again. */
	revokeRefreshTokens(): void;
	close(): Promise<void>;
}

interface Client {
	redirectUris: string[];
	secret: string | null;
}

interface Code {
	clientId: string;
	challenge: string;
	redirectUri: string;
}

function json(res: ServerResponse, status: number, body: unknown, headers = {}): void {
	res.writeHead(status, { 'content-type': 'application/json', ...headers });
	res.end(JSON.stringify(body));
}

async function body(req: IncomingMessage): Promise<string> {
	const chunks: Buffer[] = [];
	for await (const chunk of req) chunks.push(chunk as Buffer);
	return Buffer.concat(chunks).toString('utf8');
}

function token(prefix: string): string {
	return `${prefix}-${randomBytes(12).toString('hex')}`;
}

/** `preregistered`: clients an admin registered by hand, by id, with their secret. */
export async function startOAuthServer(
	options: { registration?: boolean; preregistered?: Record<string, string> } = {}
): Promise<OAuthServer> {
	const clients = new Map<string, Client>(
		Object.entries(options.preregistered ?? {}).map(([id, secret]) => [
			id,
			{ redirectUris: [], secret }
		])
	);
	const codes = new Map<string, Code>();
	let access = new Set<string>();
	let refresh = new Set<string>();
	let origin = '';

	const tokens = (status = 200) => {
		const accessToken = token('at');
		const refreshToken = token('rt');
		access.add(accessToken);
		refresh.add(refreshToken);
		return {
			status,
			body: {
				access_token: accessToken,
				refresh_token: refreshToken,
				token_type: 'Bearer',
				expires_in: 3600
			}
		};
	};

	const http = createServer(async (req, res) => {
		const url = new URL(req.url ?? '/', origin);
		const path = url.pathname;
		if (path.startsWith('/.well-known/oauth-protected-resource')) {
			json(res, 200, {
				resource: `${origin}/mcp`,
				authorization_servers: [origin],
				scopes_supported: ['notes']
			});
			return;
		}
		if (path === '/.well-known/oauth-authorization-server') {
			json(res, 200, {
				issuer: origin,
				authorization_endpoint: `${origin}/authorize`,
				token_endpoint: `${origin}/token`,
				...(options.registration !== false && { registration_endpoint: `${origin}/register` }),
				response_types_supported: ['code'],
				grant_types_supported: ['authorization_code', 'refresh_token'],
				code_challenge_methods_supported: ['S256'],
				token_endpoint_auth_methods_supported: ['none', 'client_secret_post']
			});
			return;
		}
		if (path === '/register' && req.method === 'POST' && options.registration !== false) {
			const metadata = JSON.parse(await body(req)) as { redirect_uris: string[] };
			const id = `client-${clients.size + 1}`;
			clients.set(id, { redirectUris: metadata.redirect_uris, secret: null });
			server.registered++;
			json(res, 201, { ...metadata, client_id: id, client_id_issued_at: 0 });
			return;
		}
		if (path === '/token' && req.method === 'POST') {
			const form = new URLSearchParams(await body(req));
			const client = clients.get(form.get('client_id') ?? '');
			if (!client || (client.secret && form.get('client_secret') !== client.secret)) {
				json(res, 401, { error: 'invalid_client' });
				return;
			}
			server.scopes.push(form.get('scope'));
			if (form.get('grant_type') === 'authorization_code') {
				const code = codes.get(form.get('code') ?? '');
				codes.delete(form.get('code') ?? '');
				const verifier = form.get('code_verifier') ?? '';
				const challenge = createHash('sha256').update(verifier).digest('base64url');
				if (
					!code ||
					code.clientId !== form.get('client_id') ||
					code.redirectUri !== form.get('redirect_uri') ||
					code.challenge !== challenge
				) {
					json(res, 400, { error: 'invalid_grant' });
					return;
				}
				const { status, body: answer } = tokens();
				json(res, status, answer);
				return;
			}
			if (form.get('grant_type') === 'refresh_token') {
				const given = form.get('refresh_token') ?? '';
				if (!refresh.delete(given)) {
					json(res, 400, { error: 'invalid_grant' });
					return;
				}
				const { status, body: answer } = tokens();
				json(res, status, answer);
				return;
			}
			json(res, 400, { error: 'unsupported_grant_type' });
			return;
		}
		// The sign-in page, for a browser: says yes at once, and sends it back.
		if (path === '/authorize' && req.method === 'GET') {
			const { code, state, redirectUri } = server.approve(url);
			const back = new URL(redirectUri);
			back.searchParams.set('code', code);
			back.searchParams.set('state', state);
			res.writeHead(302, { location: back.href }).end();
			return;
		}
		if (path === '/mcp') {
			const given = req.headers.authorization?.replace(/^Bearer /, '') ?? '';
			if (!access.has(given)) {
				res.writeHead(401, {
					'www-authenticate': `Bearer resource_metadata="${origin}/.well-known/oauth-protected-resource/mcp"`
				});
				res.end('Unauthorized');
				return;
			}
			// Stateless: a server of its own for every request.
			const mcp = fakeServer();
			const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined });
			res.on('close', () => {
				void transport.close();
				void mcp.close();
			});
			await mcp.connect(transport);
			await transport.handleRequest(req, res);
			return;
		}
		res.writeHead(404).end('Not found');
	});
	await new Promise<void>((resolve) => http.listen(0, '127.0.0.1', resolve));
	origin = `http://127.0.0.1:${(http.address() as AddressInfo).port}`;

	const server: OAuthServer = {
		url: `${origin}/mcp`,
		origin,
		registered: 0,
		scopes: [],
		approve(authorizationUrl) {
			const query = authorizationUrl.searchParams;
			const clientId = query.get('client_id') ?? '';
			const client = clients.get(clientId);
			const redirectUri = query.get('redirect_uri') ?? '';
			if (!client) throw new Error(`no client ${clientId}`);
			if (client.redirectUris.length && !client.redirectUris.includes(redirectUri)) {
				throw new Error(`${redirectUri} isn't one of ${clientId}'s redirects`);
			}
			if (query.get('code_challenge_method') !== 'S256') throw new Error('no PKCE');
			const code = token('code');
			codes.set(code, { clientId, challenge: query.get('code_challenge') ?? '', redirectUri });
			return { code, state: query.get('state') ?? '', redirectUri };
		},
		expireAccessTokens() {
			access = new Set();
		},
		revokeRefreshTokens() {
			refresh = new Set();
		},
		async close() {
			http.closeAllConnections();
			await new Promise((resolve) => http.close(resolve));
		}
	};
	return server;
}
