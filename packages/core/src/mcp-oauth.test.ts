import { readFileSync, statSync } from 'node:fs';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { initConfig } from './config.ts';
import {
	abandonMcpSignIn,
	callMcpTool,
	checkMcpServer,
	closeMcpConnections,
	finishMcpSignIn,
	listMcpServers,
	listMcpTools,
	removeMcpServer,
	saveMcpServer,
	signOutMcpServer,
	startMcpSignIn
} from './mcp.ts';
import { paths } from './paths.ts';
import { startOAuthServer, type OAuthServer } from './test/oauth-server.ts';

/*
 * MCP servers someone signs in to (OAuth, mcp-auth.ts), against a fake one with its own
 * authorization server: `approve` stands in for the browser.
 */

const NOLUNE = 'http://localhost:5780';

function text(result: CallToolResult): string {
	return result.content.map((c) => (c.type === 'text' ? c.text : '')).join('');
}

function state(name: string) {
	return listMcpServers().find((s) => s.name === name)?.signIn;
}

let oauth: OAuthServer;

beforeEach(async () => {
	initConfig();
	oauth = await startOAuthServer();
});

afterEach(async () => {
	await closeMcpConnections();
	await oauth.close();
});

/** Signs in to `name` as someone would, from nolune open at `origin`. */
async function signIn(name: string, origin = NOLUNE) {
	const page = await startMcpSignIn(name, origin);
	const { code, state } = oauth.approve(page);
	return finishMcpSignIn(state, code);
}

describe('a server someone signs in to', () => {
	it('says it needs a sign-in, and waits for one', async () => {
		await expect(checkMcpServer('notes', { type: 'http', url: oauth.url })).rejects.toThrow(
			'notes needs someone to sign in to it: an admin presses Sign in on the Connected services page, or runs `nolune mcp login notes`.'
		);
		saveMcpServer('notes', { type: 'http', url: oauth.url });
		expect(state('notes')).toBe('needed');
	});

	it('signs in through the browser, coming back to nolune, and then has its tools', async () => {
		saveMcpServer('notes', { type: 'http', url: oauth.url });
		const page = await startMcpSignIn('notes', NOLUNE);
		expect(page.origin).toBe(oauth.origin);
		expect(page.pathname).toBe('/authorize');
		expect(page.searchParams.get('redirect_uri')).toBe(`${NOLUNE}/mcp/oauth/callback`);
		expect(page.searchParams.get('code_challenge_method')).toBe('S256');
		expect(page.searchParams.get('scope')).toBe('notes');
		expect(oauth.registered).toBe(1);
		// Not signed in until the browser comes back.
		expect(state('notes')).toBe('needed');

		const { code, state: back } = oauth.approve(page);
		const { name, tools } = await finishMcpSignIn(back, code);
		expect(name).toBe('notes');
		expect(tools?.tools.map((t) => t.name)).toContain('echo');
		expect(state('notes')).toBe('signed-in');
		expect(text(await callMcpTool('notes', 'echo', { text: 'signed in' }))).toBe('signed in');

		// Kept for this user only, and never shown.
		expect(statSync(paths.mcpAuth).mode & 0o777).toBe(0o600);
		const kept = JSON.parse(readFileSync(paths.mcpAuth, 'utf8'));
		expect(JSON.stringify(listMcpServers())).not.toContain(kept.notes.tokens.access_token);
		// What the browser brought back works once.
		await expect(finishMcpSignIn(back, code)).rejects.toThrow(
			'this sign-in expired or was already finished'
		);
	});

	it('refreshes its tokens, and wants a sign-in again once it can’t', async () => {
		saveMcpServer('notes', { type: 'http', url: oauth.url });
		await signIn('notes');
		oauth.expireAccessTokens();
		expect(text(await callMcpTool('notes', 'echo', { text: 'refreshed' }))).toBe('refreshed');
		expect(state('notes')).toBe('signed-in');

		oauth.expireAccessTokens();
		oauth.revokeRefreshTokens();
		await expect(callMcpTool('notes', 'echo', { text: 'x' })).rejects.toThrow(
			'notes needs someone to sign in to it'
		);
		expect(state('notes')).toBe('needed');
		await signIn('notes');
		expect(text(await callMcpTool('notes', 'echo', { text: 'again' }))).toBe('again');
	});

	it('keeps a sign-in someone started when a command asks the server meanwhile', async () => {
		saveMcpServer('notes', { type: 'http', url: oauth.url });
		const page = await startMcpSignIn('notes', NOLUNE);
		await expect(listMcpTools('notes')).rejects.toThrow('notes needs someone to sign in');
		const { code, state: back } = oauth.approve(page);
		await expect(finishMcpSignIn(back, code)).resolves.toMatchObject({ name: 'notes' });
	});

	it('registers once for each address nolune is opened at, and refreshes with the one signed in from', async () => {
		saveMcpServer('notes', { type: 'http', url: oauth.url });
		await signIn('notes', NOLUNE);
		await signIn('notes', 'https://family.relay.nolune.dev');
		expect(oauth.registered).toBe(2);
		await signIn('notes', NOLUNE);
		await signIn('notes', 'https://family.relay.nolune.dev');
		expect(oauth.registered).toBe(2);
		// The gateway's connections come back to the address people open (localhost here).
		oauth.expireAccessTokens();
		expect(text(await callMcpTool('notes', 'echo', { text: 'refreshed' }))).toBe('refreshed');
		expect(oauth.registered).toBe(2);
	});

	it('uses the client an admin registered, with its secret', async () => {
		const own = await startOAuthServer({
			registration: false,
			preregistered: { 'family-app': 'shh' }
		});
		try {
			saveMcpServer('notes', {
				type: 'http',
				url: own.url,
				oauth: { clientId: 'family-app', clientSecret: 'shh' }
			});
			expect(listMcpServers()[0]).toMatchObject({
				oauthClientId: 'family-app',
				oauthSecret: true
			});
			const page = await startMcpSignIn('notes', NOLUNE);
			expect(page.searchParams.get('client_id')).toBe('family-app');
			const { code, state: back } = own.approve(page);
			await finishMcpSignIn(back, code);
			expect(state('notes')).toBe('signed-in');
			expect(own.registered).toBe(0);
		} finally {
			await closeMcpConnections();
			await own.close();
		}
	});

	it('signs out, and forgets the sign-in when its address changes or it’s removed', async () => {
		saveMcpServer('notes', { type: 'http', url: oauth.url });
		await signIn('notes');
		await signOutMcpServer('notes');
		expect(state('notes')).toBe('needed');
		await expect(callMcpTool('notes', 'echo', { text: 'x' })).rejects.toThrow(
			'needs someone to sign in'
		);

		await signIn('notes');
		saveMcpServer('notes', { type: 'http', url: `${oauth.url}?v=2` });
		expect(state('notes')).toBe(null);
		expect(JSON.parse(readFileSync(paths.mcpAuth, 'utf8')).notes).toBeUndefined();
		saveMcpServer('notes', { type: 'http', url: oauth.url });
		await signIn('notes');
		removeMcpServer('notes');
		expect(JSON.parse(readFileSync(paths.mcpAuth, 'utf8')).notes).toBeUndefined();
	});

	it('drops a sign-in the service turned down', async () => {
		saveMcpServer('notes', { type: 'http', url: oauth.url });
		const page = await startMcpSignIn('notes', NOLUNE);
		const { code, state: back } = oauth.approve(page);
		expect(abandonMcpSignIn('made-up')).toBeNull();
		expect(abandonMcpSignIn(back)).toBe('notes');
		await expect(finishMcpSignIn(back, code)).rejects.toThrow('expired or was already finished');
	});

	it('says what went wrong with a sign-in', async () => {
		await expect(finishMcpSignIn('made-up', 'code')).rejects.toThrow(
			'expired or was already finished'
		);
		saveMcpServer('notes', { type: 'http', url: oauth.url });
		const page = await startMcpSignIn('notes', NOLUNE);
		const { state: back } = oauth.approve(page);
		await expect(finishMcpSignIn(back, 'not-the-code')).rejects.toThrow(
			/couldn't sign in to notes/
		);
		saveMcpServer('home', { type: 'stdio', command: 'home-mcp' });
		await expect(startMcpSignIn('home')).rejects.toThrow(
			'home runs on this computer: it gets its keys as environment variables'
		);
	});
});
