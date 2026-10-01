import { error, fail } from '@sveltejs/kit';
import {
	McpServerError,
	checkMcpServer,
	describeFromServer,
	findMcpServer,
	isMcpAddress,
	listMcpServers,
	listProfiles,
	mcpServerNameProblem,
	parseMcpServer,
	removeMcpServer,
	saveMcpServer,
	splitCommandLine,
	type McpServerConfig,
	type McpServerTools
} from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireAdmin } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	requireAdmin(locals);
	return {
		// Each server's command or address and the names of its keys, never the keys.
		servers: listMcpServers(),
		profiles: listProfiles().map((p) => ({ slug: p.slug, name: p.name }))
	};
};

/** A message from core at the start of a sentence. */
function sentence(text: string): string {
	return `${text[0]?.toUpperCase() ?? ''}${text.slice(1)}`;
}

/** A form's keys: a line each, `NAME=value` or `Name: value`; or the first line that isn't. */
function secretLines(
	text: string,
	separator: '=' | ':'
): { map: Record<string, string> } | { bad: string } {
	const map: Record<string, string> = {};
	for (const line of text.split('\n').map((l) => l.trim())) {
		if (!line) continue;
		const at = line.indexOf(separator);
		if (at <= 0) return { bad: line.length > 40 ? `${line.slice(0, 40)}…` : line };
		map[line.slice(0, at).trim()] = line.slice(at + 1).trim();
	}
	return { map };
}

/** A few of a server's tools, by name, for a sentence. */
function someTools(found: McpServerTools): string {
	const names = found.tools.map((t) => t.name);
	return names.length > 6 ? `${names.slice(0, 6).join(', ')}…` : names.join(', ');
}

/** The saved server a form names, or a 400. */
async function named(request: Request): Promise<string> {
	const name = (await request.formData()).get('name')?.toString() ?? '';
	if (!listMcpServers().some((s) => s.name === name)) error(400, 'Unknown MCP server');
	return name;
}

export const actions: Actions = {
	/**
	 * Connects a server (no `editing`), or changes the one `editing` names, after connecting to it
	 * to check it; one it can't connect to is saved with why. `mcpServer` in the result says which
	 * row it's for, or the add form with `''`.
	 */
	save: async ({ locals, request }) => {
		requireAdmin(locals);
		const t = translations(locals.locale).m.services;
		const form = await request.formData();
		const editing = form.get('editing')?.toString() || undefined;
		const name = editing ?? form.get('name')?.toString().trim() ?? '';
		const refuse = (mcpError: string) => fail(400, { mcpServer: editing ?? '', mcpError });
		const current = editing ? listMcpServers().find((s) => s.name === editing) : undefined;
		if (editing && !current) error(400, 'Unknown MCP server');
		if (!editing) {
			const problem = mcpServerNameProblem(name);
			if (problem) return refuse(problem);
			if (listMcpServers().some((s) => s.name === name)) return refuse(t.taken(name));
		}
		const slugs = new Set(listProfiles().map((p) => p.slug));
		const profiles = form.getAll('profiles').map(String);
		if (profiles.some((slug) => !slugs.has(slug))) error(400, 'Unknown profile');
		const description = form.get('description')?.toString().trim() || undefined;
		const secrets = form.get('secrets')?.toString().trim() ?? '';
		// The saved one, keys and all: a form that leaves its keys empty keeps them.
		let saved: McpServerConfig | null = null;
		try {
			saved = editing ? findMcpServer(editing) : null;
		} catch (err) {
			if (!(err instanceof McpServerError)) throw err;
		}
		const kind = form.get('kind')?.toString();
		let server: McpServerConfig;
		if (kind === 'stdio') {
			let words: string[];
			try {
				words = splitCommandLine(form.get('command')?.toString() ?? '');
			} catch (err) {
				if (!(err instanceof McpServerError)) throw err;
				return refuse(err.message);
			}
			if (!words.length) return refuse(t.needCommand);
			const env = secretLines(secrets, '=');
			if ('bad' in env) return refuse(t.badEnv(env.bad));
			const kept = saved?.type === 'stdio' ? saved : null;
			server = {
				type: 'stdio',
				command: words[0],
				args: words.slice(1),
				env: secrets ? env.map : kept?.env,
				// Only the CLI sets where it starts; it stays.
				cwd: kept?.cwd,
				description,
				profiles
			};
		} else if (kind === 'remote') {
			const url = form.get('url')?.toString().trim() ?? '';
			if (!isMcpAddress(url)) return refuse(t.needAddress);
			const headers = secretLines(secrets, ':');
			if ('bad' in headers) return refuse(t.badHeader(headers.bad));
			const kept = saved && saved.type !== 'stdio' ? saved : null;
			server = {
				type: form.get('transport')?.toString() === 'sse' ? 'sse' : 'http',
				url,
				headers: secrets ? headers.map : kept?.headers,
				description,
				profiles
			};
		} else error(400, 'Unknown kind of MCP server');
		let parsed: McpServerConfig;
		try {
			parsed = parseMcpServer(server);
		} catch (err) {
			if (!(err instanceof McpServerError)) throw err;
			return refuse(err.message);
		}
		let found: McpServerTools | null = null;
		let problem = '';
		try {
			found = await checkMcpServer(name, parsed);
		} catch (err) {
			if (!(err instanceof McpServerError)) throw err;
			problem = err.message;
		}
		if (found && !parsed.description) parsed.description = describeFromServer(found);
		saveMcpServer(name, parsed);
		return found
			? { mcpServer: name, mcpMessage: t.works(found.tools.length, someTools(found)) }
			: { mcpServer: name, mcpWarning: t.unchecked(sentence(problem)) };
	},
	/** Connects to a saved server, to see that it works. */
	check: async ({ locals, request }) => {
		requireAdmin(locals);
		const t = translations(locals.locale).m.services;
		const name = await named(request);
		try {
			const found = await checkMcpServer(name, findMcpServer(name));
			return { mcpServer: name, mcpMessage: t.works(found.tools.length, someTools(found)) };
		} catch (err) {
			if (!(err instanceof McpServerError)) throw err;
			return fail(400, { mcpServer: name, mcpError: sentence(err.message) });
		}
	},
	remove: async ({ locals, request }) => {
		requireAdmin(locals);
		removeMcpServer(await named(request));
		return { mcpServer: '', mcpMessage: translations(locals.locale).m.services.removed };
	}
};
