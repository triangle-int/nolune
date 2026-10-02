import { parseArgs } from 'node:util';
import {
	McpServerError,
	callMcpTool,
	checkMcpServer,
	describeFromServer,
	getProfileBySlug,
	isMcpAddress,
	listMcpServers,
	listMcpTools,
	mcpResultText,
	mcpServerNameProblem,
	parseMcpServer,
	removeMcpServer,
	saveMcpServer,
	type McpServerConfig,
	type McpServerTools
} from '@nolune/core';
import { fail, type Io } from './io.ts';

type Tool = McpServerTools['tools'][number];
type ToolResult = Awaited<ReturnType<typeof callMcpTool>>;

export const MCP_HELP = `MCP servers (other apps' and services' tools, which chats get next to run_command)
  nolune mcp add <name> <url> [--transport http|sse] [--header "Name: value"]...
                 [--description D] [--profile SLUG]...
  nolune mcp add <name> [--env NAME=value]... [--cwd DIR] [--description D] [--profile SLUG]...
                 -- <command> [args...]
                                             connect a server at an address, or one this computer
                                             runs (found on the PATH the agent's commands have);
                                             nolune connects to check it. The same name again
                                             changes it. --profile: only in those profiles
                                             (in every profile without it)
  nolune mcp add-json <name> '<json>'           the same, as MCP clients' settings write a server:
                                             {"command": …, "args": […], "env": {…}} or
                                             {"type": "http", "url": …, "headers": {…}}
  nolune mcp rm <name>
  nolune mcp list                               the servers (and the names of their keys, never
                                             the keys)
  nolune mcp tools [<server> [<tool>]]          their tools; with a tool, what it does and takes
  nolune mcp call <server> <tool> [<json>|-]    call a tool with a JSON object of arguments (- reads
                                             stdin); pictures it returns are attached for the agent`;

const USAGE = 'usage: nolune mcp add|add-json|rm|list|tools|call. See `nolune help`.';

/** How much of a tool's description the list shows, and of a server's instructions. */
const SHORT_DESCRIPTION = 200;
const MAX_INSTRUCTIONS = 4000;

/** The profiles a server is for, by slug, each of which must exist. */
function profiles(given: string[] | undefined): string[] | undefined {
	for (const slug of given ?? []) {
		if (!getProfileBySlug(slug))
			fail(`no profile with slug "${slug}". See \`nolune profile list\`.`);
	}
	return given?.length ? given : undefined;
}

/** `NAME=value` and `Name: value` pairs, as typed. */
function pairs(
	given: string[] | undefined,
	separator: '=' | ':',
	what: string
): Record<string, string> {
	const map: Record<string, string> = {};
	for (const pair of given ?? []) {
		const at = pair.indexOf(separator);
		if (at <= 0)
			fail(
				`${what} is given as ${separator === '=' ? 'NAME=value' : '"Name: value"'}, not "${pair}"`
			);
		map[pair.slice(0, at).trim()] = pair.slice(at + 1).trim();
	}
	return map;
}

/**
 * Checks a server by connecting to it, and saves it: a server it can't connect to is saved all
 * the same, with why, since what it needs may come later (a program installed, a server started).
 */
async function store(io: Io, name: string, server: McpServerConfig): Promise<void> {
	const problem = mcpServerNameProblem(name);
	if (problem) fail(problem);
	let parsed: McpServerConfig;
	try {
		parsed = parseMcpServer(server);
	} catch (err) {
		fail((err as Error).message);
	}
	let found: McpServerTools | null = null;
	let unreachable = '';
	try {
		found = await checkMcpServer(name, parsed, io.signal);
	} catch (err) {
		if (!(err instanceof McpServerError)) throw err;
		unreachable = err.message;
	}
	if (found && !parsed.description) parsed.description = describeFromServer(found);
	const { replaced } = saveMcpServer(name, parsed);
	const done = replaced ? 'Changed' : 'Added';
	if (!found) {
		io.log(`${done} ${name}, but couldn't connect to it: ${unreachable}`);
		return;
	}
	const names = found.tools.map((t) => t.name);
	const some = names.slice(0, 8).join(', ');
	const more = names.length > 8 ? ` and ${names.length - 8} more` : '';
	io.log(
		`${done} ${name}: ${names.length} ${names.length === 1 ? 'tool' : 'tools'}${names.length ? ` (${some}${more})` : ''}.${parsed.profiles ? ` In ${parsed.profiles.join(', ')} only.` : ''}`
	);
}

async function add(io: Io, args: string[]): Promise<void> {
	const split = args.indexOf('--');
	const own = split === -1 ? args : args.slice(0, split);
	let parsed: ReturnType<typeof parseAdd>;
	try {
		parsed = parseAdd(own);
	} catch (err) {
		// `nolune mcp add files npx -y …`: the server's own flags look like nolune's.
		if ((err as { code?: string }).code !== 'ERR_PARSE_ARGS_UNKNOWN_OPTION') throw err;
		fail(
			`put the server's command after --, so its options aren't taken for nolune's: nolune mcp add <name> [options] -- <command> [args...]`
		);
	}
	const { values, positionals } = parsed;
	const [name, ...rest] = positionals;
	if (!name)
		fail('usage: nolune mcp add <name> <url> | nolune mcp add <name> -- <command> [args...]');
	const words = [...rest, ...(split === -1 ? [] : args.slice(split + 1))];
	if (!words.length) fail(`what does ${name} run, or where is it? See \`nolune help\`.`);
	const shared = { description: values.description, profiles: profiles(values.profile) };
	const transport = values.transport;
	if (words.length === 1 && isMcpAddress(words[0])) {
		if (values.env || values.cwd) fail('--env and --cwd are for a server this computer runs.');
		if (transport !== undefined && transport !== 'http' && transport !== 'sse') {
			fail('--transport is http (the default) or sse, for older servers.');
		}
		const headers = pairs(values.header, ':', 'A header');
		await store(io, name, {
			type: transport === 'sse' ? 'sse' : 'http',
			url: words[0],
			...(Object.keys(headers).length && { headers }),
			...shared
		});
		return;
	}
	if (values.header) fail('--header is for a server at an address (http:// or https://).');
	if (transport !== undefined && transport !== 'stdio') {
		fail(`${words[0]} isn't an address: a server at one starts with http:// or https://.`);
	}
	const env = pairs(values.env, '=', 'An environment variable');
	await store(io, name, {
		type: 'stdio',
		command: words[0],
		...(words.length > 1 && { args: words.slice(1) }),
		...(Object.keys(env).length && { env }),
		...(values.cwd && { cwd: values.cwd }),
		...shared
	});
}

function parseAdd(args: string[]) {
	return parseArgs({
		args,
		allowPositionals: true,
		options: {
			transport: { type: 'string', short: 't' },
			header: { type: 'string', short: 'H', multiple: true },
			env: { type: 'string', short: 'e', multiple: true },
			cwd: { type: 'string' },
			description: { type: 'string', short: 'd' },
			profile: { type: 'string', multiple: true }
		}
	});
}

async function addJson(io: Io, args: string[]): Promise<void> {
	const [name, json] = args;
	if (!name || !json) fail(`usage: nolune mcp add-json <name> '<json>' (- reads stdin)`);
	const text = json === '-' ? await io.readStdin() : json;
	let value: unknown;
	try {
		value = JSON.parse(text);
	} catch (err) {
		fail(`that isn't JSON: ${(err as Error).message}`);
	}
	// A whole `{"mcpServers": {...}}` from a README, with this one server in it.
	const wrapped = (value as { mcpServers?: unknown } | null)?.mcpServers;
	if (wrapped && typeof wrapped === 'object') {
		const inside = Object.values(wrapped);
		if (inside.length !== 1) fail('give one server: the object inside "mcpServers".');
		value = inside[0];
	}
	if (value && typeof value === 'object' && 'profiles' in value) {
		profiles((value as { profiles?: string[] }).profiles);
	}
	await store(io, name, value as McpServerConfig);
}

function list(io: Io): void {
	const profile = io.env.NOLUNE_PROFILE;
	const servers = listMcpServers(profile);
	if (!servers.length) {
		io.log(
			`No MCP servers${profile ? ' in this profile' : ''}. An admin connects them on the Connected services page, or with \`nolune mcp add\`.`
		);
		return;
	}
	for (const s of servers) {
		if (s.problem) {
			io.log(`${s.name}\tbroken settings: ${s.problem}`);
			continue;
		}
		const kind = s.type === 'stdio' ? 'env' : 'headers';
		const keys = s.secrets.length ? `${kind}: ${s.secrets.join(', ')}` : `no ${kind}`;
		const where = s.profiles ? `profiles: ${s.profiles.join(', ')}` : 'every profile';
		io.log(`${s.name}\t${s.type}\t${s.target}\t${keys}\t${where}`);
		if (s.description) io.log(`  ${s.description}`);
	}
}

/** `name(a, b?)`: a tool's arguments, the optional ones marked. */
function signature(tool: Tool): string {
	const properties = Object.keys(tool.inputSchema.properties ?? {});
	const required = new Set(tool.inputSchema.required ?? []);
	const shown = properties.slice(0, 8).map((p) => (required.has(p) ? p : `${p}?`));
	if (properties.length > 8) shown.push('…');
	return `${tool.name}(${shown.join(', ')})`;
}

/** What a tool says it does to things, when it says. */
function tags(tool: Tool): string {
	const hints = tool.annotations;
	if (hints?.readOnlyHint) return ' [reads only]';
	if (hints?.destructiveHint) return ' [can delete or overwrite]';
	return '';
}

function clip(text: string, max: number): string {
	return text.length > max ? `${text.slice(0, max).trimEnd()}…` : text;
}

function printTools(io: Io, name: string, found: McpServerTools, instructions: boolean): void {
	const n = found.tools.length;
	const title = found.title && found.title !== name ? `${found.title}, ` : '';
	io.log(`${name}: ${title}${n} ${n === 1 ? 'tool' : 'tools'}`);
	if (instructions && found.instructions) io.log(clip(found.instructions, MAX_INSTRUCTIONS));
	for (const tool of found.tools) {
		io.log(`\n${signature(tool)}${tags(tool)}`);
		const first = tool.description?.trim().split('\n')[0];
		if (first) io.log(`  ${clip(first, SHORT_DESCRIPTION)}`);
	}
}

async function tools(io: Io, args: string[]): Promise<number> {
	const [server, toolName] = args;
	const options = { profile: io.env.NOLUNE_PROFILE, signal: io.signal };
	if (server) {
		const found = await listMcpTools(server, options);
		if (!toolName) {
			printTools(io, server, found, true);
			return 0;
		}
		const tool = found.tools.find((t) => t.name === toolName);
		if (!tool)
			fail(`${server} has no tool called ${toolName}. \`nolune mcp tools ${server}\` lists them.`);
		const title = tool.title ?? tool.annotations?.title;
		io.log(`${server} ${tool.name}${title ? `: ${title}` : ''}${tags(tool)}`);
		if (tool.description?.trim()) io.log(tool.description.trim());
		io.log(`\nArguments (JSON Schema):\n${JSON.stringify(tool.inputSchema, null, 2)}`);
		if (tool.outputSchema) {
			io.log(
				`\nIts structured result (JSON Schema):\n${JSON.stringify(tool.outputSchema, null, 2)}`
			);
		}
		return 0;
	}
	const servers = listMcpServers(options.profile).filter((s) => !s.problem);
	if (!servers.length) {
		list(io);
		return 0;
	}
	// Each server answers on its own: one that's down doesn't hide the others' tools.
	const answers = await Promise.allSettled(servers.map((s) => listMcpTools(s.name, options)));
	let failed = false;
	for (const [i, answer] of answers.entries()) {
		if (i) io.log('');
		if (answer.status === 'fulfilled') printTools(io, servers[i].name, answer.value, false);
		else {
			failed = true;
			io.error(`nolune: ${(answer.reason as Error).message ?? answer.reason}`);
		}
	}
	if (servers.length > 1 && answers.some((a) => a.status === 'fulfilled')) {
		io.log('\n`nolune mcp tools <server>` also shows what a server says about using it.');
	}
	return failed ? 1 : 0;
}

async function call(io: Io, args: string[]): Promise<number> {
	const [server, tool, json, ...extra] = args;
	if (!server || !tool || extra.length) {
		fail(`usage: nolune mcp call <server> <tool> ['<json>'|-], the arguments one JSON object`);
	}
	const text = json === '-' ? await io.readStdin() : (json ?? '');
	let input: unknown = {};
	if (text.trim()) {
		try {
			input = JSON.parse(text);
		} catch (err) {
			fail(
				`the arguments aren't JSON (${(err as Error).message}). Give one object, like '{"query": "…"}'.`
			);
		}
	}
	if (typeof input !== 'object' || input === null || Array.isArray(input)) {
		fail(`the arguments are one JSON object, like '{"query": "…"}'.`);
	}
	let result: ToolResult;
	try {
		result = await callMcpTool(server, tool, input as Record<string, unknown>, {
			profile: io.env.NOLUNE_PROFILE,
			signal: io.signal
		});
	} catch (err) {
		if (err instanceof McpServerError) throw err;
		// The server turned the call down: an unknown tool, or arguments it doesn't take.
		fail(
			`${server} ${tool}: ${(err as Error).message}. \`nolune mcp tools ${server} ${tool}\` shows what it takes.`
		);
	}
	const output = await mcpResultText(result, io.env.NOLUNE_VIEW_DIR);
	if (output) io.log(output);
	if (result.isError) {
		io.error(`nolune: ${server} ${tool} reported an error.`);
		return 1;
	}
	return 0;
}

/** `nolune mcp`: the exit code. */
export async function mcpCommand(
	io: Io,
	action: string | undefined,
	args: string[]
): Promise<number | void> {
	switch (action) {
		case 'add':
			return add(io, args);
		case 'add-json':
			return addJson(io, args);
		case 'rm': {
			const [name] = args;
			if (!name) fail('usage: nolune mcp rm <name>');
			removeMcpServer(name);
			io.log(`Removed ${name}.`);
			return;
		}
		case undefined:
		case 'list':
			return list(io);
		case 'tools':
			return tools(io, args);
		case 'call':
			return call(io, args);
		default:
			fail(USAGE);
	}
}
