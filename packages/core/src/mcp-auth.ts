import { randomBytes } from 'node:crypto';
import { chmodSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import type {
	OAuthClientProvider,
	OAuthDiscoveryState
} from '@modelcontextprotocol/sdk/client/auth.js';
import type {
	OAuthClientInformationMixed,
	OAuthClientMetadata,
	OAuthTokens
} from '@modelcontextprotocol/sdk/shared/auth.js';
import { DEFAULT_PORT, publicOrigin, readConfig } from './config.ts';
import type { McpRemoteServer } from './mcp.ts';
import { paths } from './paths.ts';

/*
 * Signing in to MCP servers at an address with OAuth, as MCP has it: the server says where its
 * authorization server is, nolune registers there as a client of its own (or uses one an admin
 * registered, `oauth.clientId`), and someone signs in in a browser, which comes back to nolune's
 * web address with a code (PKCE) that nolune trades for tokens. The MCP SDK does the protocol
 * (`auth`); this is where nolune keeps what it needs (McpSignIn, an OAuthClientProvider) and the
 * tokens, in mcp-auth.json, readable by this user only. A server's sign-in is the server's: every
 * profile that has it uses the account someone signed in with, so a server that reaches someone's
 * own account belongs in their profile.
 */

/** Where the authorization server sends the browser back to, on nolune's web address. */
export const MCP_SIGN_IN_PATH = '/mcp/oauth/callback';

/** How long a sign-in someone started can be finished. */
const SIGN_IN_TTL_MS = 15 * 60_000;

/** One server's sign-in, by the server's name. */
interface ServerAuth {
	/** The address it's for: a server whose address changed starts again. */
	url: string;
	/**
	 * nolune as a client of the server's authorization server, by the redirect each registration
	 * is for: nolune opened at another address registers again.
	 */
	clients?: Record<string, OAuthClientInformationMixed>;
	/** Where its authorization server is, and what it says of itself. */
	discovery?: OAuthDiscoveryState;
	tokens?: OAuthTokens;
	/** The client the tokens were given to, which refreshes them. */
	tokensClient?: OAuthClientInformationMixed;
	/** When `tokens` were last saved. */
	savedAt?: number;
	/** The server asked for a sign-in nolune doesn't have, or that stopped working. */
	needed?: boolean;
	/** A sign-in someone started: what the browser must bring back, and the PKCE verifier. */
	pending?: { state: string; codeVerifier: string; redirectUrl: string; startedAt: number };
}

function readAll(): Record<string, ServerAuth> {
	try {
		return JSON.parse(readFileSync(paths.mcpAuth, 'utf8')) as Record<string, ServerAuth>;
	} catch {
		return {};
	}
}

/** Written whole and renamed into place, so another process never reads half of it. */
function writeAll(all: Record<string, ServerAuth>): void {
	mkdirSync(dirname(paths.mcpAuth), { recursive: true });
	const temporary = `${paths.mcpAuth}.${process.pid}.tmp`;
	writeFileSync(temporary, `${JSON.stringify(all, null, '\t')}\n`, { mode: 0o600 });
	chmodSync(temporary, 0o600);
	renameSync(temporary, paths.mcpAuth);
}

/** The server's sign-in, as long as it's for the address it has now. */
function entry(name: string, url: string): ServerAuth {
	const found = readAll()[name];
	return found?.url === url ? found : { url };
}

function update(name: string, url: string, change: (auth: ServerAuth) => void): void {
	const all = readAll();
	const auth = all[name]?.url === url ? all[name] : { url };
	change(auth);
	all[name] = auth;
	writeAll(all);
}

/** Forgets a server's sign-in: it was removed, or now has another address. */
export function forgetMcpSignIn(name: string): void {
	const all = readAll();
	if (!Object.hasOwn(all, name)) return;
	delete all[name];
	writeAll(all);
}

/**
 * Forgets the server's tokens (signing out), keeping nolune's registration with its authorization
 * server for the next sign-in.
 */
export function signOutOf(name: string, url: string): void {
	update(name, url, (auth) => {
		delete auth.tokens;
		delete auth.tokensClient;
		delete auth.savedAt;
		delete auth.pending;
		auth.needed = true;
	});
}

/**
 * Whether the server at `url` is signed in to (`signed-in`), wants a sign-in (`needed`: it asked
 * for one, or someone signed out), or signs in some other way, as far as nolune knows (null).
 */
export function mcpSignInState(name: string, url: string): 'signed-in' | 'needed' | null {
	const auth = entry(name, url);
	if (auth.tokens && !auth.needed) return 'signed-in';
	return auth.needed || auth.tokens || auth.discovery ? 'needed' : null;
}

/** The sign-in someone started that `state` is from, while it can still be finished. */
export function pendingSignIn(
	state: string
): { name: string; url: string; redirectUrl: string } | null {
	if (!state) return null;
	for (const [name, auth] of Object.entries(readAll())) {
		const pending = auth.pending;
		if (pending?.state !== state) continue;
		if (Date.now() - pending.startedAt > SIGN_IN_TTL_MS) return null;
		return { name, url: auth.url, redirectUrl: pending.redirectUrl };
	}
	return null;
}

/** Drops a sign-in someone started (the service turned it down): its state no longer works. */
export function dropPendingSignIn(name: string): void {
	const all = readAll();
	if (!all[name]?.pending) return;
	delete all[name].pending;
	writeAll(all);
}

/**
 * The address the browser comes back to after signing in: `origin` (where someone has nolune
 * open), or the address people open it at (publicOrigin).
 */
export function signInRedirectUrl(origin?: string): string {
	let base = origin;
	if (!base) {
		try {
			base = publicOrigin(readConfig());
		} catch {
			// not set up yet
			base = `http://localhost:${DEFAULT_PORT}`;
		}
	}
	return new URL(MCP_SIGN_IN_PATH, base).href;
}

/**
 * nolune's side of a server's OAuth, for the MCP SDK. `interactive`: someone is signing in, so a
 * sign-in it starts is kept for the browser to finish, and the tokens saved are ignored (it's a
 * new sign-in). Otherwise it's a connection, which uses the tokens and refreshes them, and only
 * notes that the server wants a sign-in: it must never replace one someone is in the middle of.
 */
export class McpSignIn implements OAuthClientProvider {
	/** The authorization server's page to sign in at, once the SDK asked for one. */
	authorizationUrl: URL | null = null;
	/** Whether the SDK went as far as looking for the server's authorization server. */
	tried = false;
	/** Whether it found one: a server without one wants a key instead. */
	offersSignIn = false;
	private readonly name: string;
	private readonly server: McpRemoteServer;
	private readonly redirect: string;
	private readonly interactive: boolean;
	private stateValue: string | null = null;
	private verifier: string | null = null;

	constructor(name: string, server: McpRemoteServer, redirect: string, interactive: boolean) {
		this.name = name;
		this.server = server;
		this.redirect = redirect;
		this.interactive = interactive;
	}

	get redirectUrl(): string {
		return this.redirect;
	}

	get clientMetadata(): OAuthClientMetadata {
		const { oauth } = this.server;
		return {
			client_name: 'nolune',
			redirect_uris: [this.redirect],
			grant_types: ['authorization_code', 'refresh_token'],
			response_types: ['code'],
			token_endpoint_auth_method: oauth?.clientSecret ? 'client_secret_post' : 'none',
			...(oauth?.scope && { scope: oauth.scope })
		};
	}

	private get saved(): ServerAuth {
		return entry(this.name, this.server.url);
	}

	private change(change: (auth: ServerAuth) => void): void {
		update(this.name, this.server.url, change);
	}

	state(): string {
		return (this.stateValue ??= randomBytes(24).toString('base64url'));
	}

	clientInformation(): OAuthClientInformationMixed | undefined {
		const { oauth } = this.server;
		if (oauth?.clientId) {
			return {
				client_id: oauth.clientId,
				...(oauth.clientSecret && { client_secret: oauth.clientSecret })
			};
		}
		const saved = this.saved;
		// Tokens are refreshed by the client they were given to, wherever someone signed in from.
		if (!this.interactive && saved.tokens && saved.tokensClient) return saved.tokensClient;
		return saved.clients?.[this.redirect];
	}

	saveClientInformation(info: OAuthClientInformationMixed): void {
		this.change((auth) => {
			auth.clients = { ...auth.clients, [this.redirect]: info };
		});
	}

	tokens(): OAuthTokens | undefined {
		return this.interactive ? undefined : this.saved.tokens;
	}

	saveTokens(tokens: OAuthTokens): void {
		const client = this.clientInformation();
		this.change((auth) => {
			auth.tokens = tokens;
			if (client) auth.tokensClient = client;
			auth.savedAt = Date.now();
			delete auth.needed;
			delete auth.pending;
		});
	}

	saveCodeVerifier(verifier: string): void {
		this.verifier = verifier;
	}

	codeVerifier(): string {
		const verifier = this.verifier ?? this.saved.pending?.codeVerifier;
		if (!verifier) throw new Error('No sign-in was started for this server.');
		return verifier;
	}

	redirectToAuthorization(url: URL): void {
		this.authorizationUrl = url;
		if (this.interactive && this.verifier) {
			const pending = {
				state: this.state(),
				codeVerifier: this.verifier,
				redirectUrl: this.redirect,
				startedAt: Date.now()
			};
			this.change((auth) => {
				auth.pending = pending;
			});
		} else {
			this.change((auth) => {
				auth.needed = true;
			});
		}
	}

	invalidateCredentials(scope: 'all' | 'client' | 'tokens' | 'verifier' | 'discovery'): void {
		this.change((auth) => {
			const all = scope === 'all';
			if (all) delete auth.clients;
			else if (scope === 'client' && auth.clients) delete auth.clients[this.redirect];
			if (all || scope === 'client' || scope === 'tokens') {
				delete auth.tokens;
				delete auth.tokensClient;
				delete auth.savedAt;
			}
			if (all || scope === 'verifier') delete auth.pending;
			if (all || scope === 'discovery') delete auth.discovery;
		});
	}

	discoveryState(): OAuthDiscoveryState | undefined {
		this.tried = true;
		const discovery = this.saved.discovery;
		if (discovery) this.offersSignIn = true;
		return discovery;
	}

	saveDiscoveryState(state: OAuthDiscoveryState): void {
		// Found nothing: the SDK guesses the server's own address, which a server that wants a
		// key doesn't answer at. Nothing to keep.
		if (!state.resourceMetadata && !state.authorizationServerMetadata) return;
		this.offersSignIn = true;
		this.change((auth) => {
			auth.discovery = state;
		});
	}
}
