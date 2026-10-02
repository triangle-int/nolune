import { redirect } from '@sveltejs/kit';
import { McpServerError, abandonMcpSignIn, finishMcpSignIn } from '@nolune/core';
import type { PageServerLoad } from './$types';

/** A message from core, or the service, at the start of a sentence. */
function sentence(text: string | null): string | null {
	return text ? `${text[0].toUpperCase()}${text.slice(1)}`.replace(/([^.!?])$/, '$1.') : null;
}

/**
 * Where an MCP server's sign-in comes back to (MCP_SIGN_IN_PATH), with the code to trade for its
 * tokens and the state nolune gave it. An admin goes back to Connected services, which says how
 * it went; anyone else (a browser `nolune mcp login` opened) sees it here.
 */
export const load: PageServerLoad = async ({ url, locals }) => {
	const state = url.searchParams.get('state') ?? '';
	const code = url.searchParams.get('code');
	const denied = url.searchParams.get('error');
	if (!code) {
		// Only what the service says of a sign-in nolune started: this page is open to anyone.
		const ours = abandonMcpSignIn(state) !== null;
		const said = ours ? url.searchParams.get('error_description') : null;
		return { name: null, tools: null, problem: sentence(said), denied: ours && !!denied };
	}
	let done: Awaited<ReturnType<typeof finishMcpSignIn>>;
	try {
		done = await finishMcpSignIn(state, code);
	} catch (err) {
		if (!(err instanceof McpServerError)) throw err;
		return { name: null, tools: null, problem: sentence(err.message), denied: false };
	}
	const tools = done.tools?.tools.length ?? null;
	if (locals.user?.isAdmin) {
		const back = new URLSearchParams({ signedIn: done.name });
		if (tools !== null) back.set('tools', String(tools));
		redirect(303, `/admin/services?${back}`);
	}
	return { name: done.name, tools, problem: null, denied: false };
};
