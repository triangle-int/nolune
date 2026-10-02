import { existsSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { userInfo } from 'node:os';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import {
	API_KEYS,
	ApiKeyError,
	CustomProviderError,
	configuredImageModel,
	DEFAULT_PORT,
	INVITE_DAYS,
	MAX_MEDIA_BYTES,
	addPreset,
	appManaged,
	editPreset,
	apiKeyStatuses,
	checkApiKey,
	checkCustomProvider,
	chatGptPlanStatus,
	nolunePlanStatus,
	claudeExecutable,
	configExists,
	findCustomProvider,
	createSkill,
	createInvite,
	createUser,
	deleteUser,
	describeCommandSafety,
	describeUpdates,
	effectiveContextWindow,
	findPreset,
	forgetRelease,
	idleCompactionChanged,
	idleCompactionMinutes,
	generatePassword,
	getDb,
	getDefaultPreset,
	getProfileBySlug,
	embeddingProblem,
	embeddingStatus,
	imageGenerationStatus,
	parseEmbeddingSetting,
	saveEmbeddingSetting,
	initConfig,
	installCliShim,
	isApiKeyProvider,
	isCommandMode,
	isCustomProvider,
	isProvider,
	providerFor,
	listPresets,
	listProfileSkills,
	listCustomProviders,
	listUsers,
	normalizeApiKey,
	parseImageModel,
	paths,
	publicOrigin,
	profileSkillsDir,
	readConfig,
	removeApiKey,
	removeCustomProvider,
	removePreset,
	saveApiKey,
	saveCommandMode,
	saveIdleCompaction,
	saveCustomProvider,
	saveSafetyPreset,
	scanSkills,
	setAdmin,
	setDefaultPreset,
	setPassword,
	setSkillsEnabled,
	splitModel,
	updateConfig,
	userPictureFile,
	viewImage,
	ViewLimitError,
	type ApiKeyProvider,
	type CustomApi
} from '@nolune/core';
import { AGENT_HELP, agentCommand } from './agent.ts';
import { CARD_HELP, cardCommand } from './card.ts';
import { generateCommand, generateHelp } from './generate.ts';
import { ask, askHidden } from './input.ts';
import { fail, type Io } from './io.ts';
import { planCommand, requireClaudePlan } from './plans.ts';
import { MCP_HELP, mcpCommand } from './mcp.ts';
import { MEMORY_HELP, memoryCommand } from './memory.ts';
import { PROFILE_HELP, profileCommand } from './profile.ts';
import { RELAY_HELP, RelayUnreachable, connectRelay, enableRelay, relayCommand } from './relay.ts';
import { SOUL_HELP, soulCommand } from './soul.ts';
import { TRIGGER_HELP, triggerCommand, wakeCommand } from './triggers.ts';
import {
	installService,
	logFile,
	renderServiceFile,
	restartService,
	serviceStatus,
	uninstallService
} from './service.ts';
import { WEB_HELP, webCommand } from './web.ts';

/** Built when shown, like generateHelp(): the gateway serves it for as long as it runs. */
const help = () => `nolune - a family agent that runs on this computer

Getting started
  nolune setup                               interactive first-time setup (your account, and the
                                             address the family opens: nolune's relay, this
                                             computer, or your own URL); the model comes after: a
                                             new profile's welcome on the web asks for one, or use
                                             key set and preset add below
  nolune start                               run the gateway in the foreground
  nolune service install|uninstall|restart|status|logs [-f]
                                             run it in the background (macOS, or Linux with systemd)

${RELAY_HELP}

Settings (${paths.home})
  nolune config                                 show address, port and what's configured
  nolune config set <host|port|origin> <value>  origin = the public URL people open (the relay's
                                             address is used instead while the relay is on)
  nolune config set image-model <provider/model>  for pictures, e.g. openai/gpt-image-2.5-flare
  nolune config set embeddings <auto|off|provider/model>
                                             what memory search finds meaning with: auto uses the
                                             OpenAI key, else OpenRouter's; the provider is openai,
                                             openrouter or custom-openai (a custom provider's model,
                                             like custom-openai/ollama/nomic-embed-text)
  nolune config set claude-path <path>          the Claude Code that claude-plan chats run (found on
                                             the PATH and in its usual folders otherwise)
  nolune config set command-mode <auto|unrestricted>
                                             auto (the default): a model checks each command the
                                             agent runs and blocks what could do harm nobody asked
                                             for; unrestricted runs them unchecked (not
                                             recommended). Not from the agent's own commands
  nolune config set safety-model <preset|chat>  the preset whose model does auto mode's checks, or
                                             chat for each chat's own model (the default)
  nolune config set update-check <on|off>       on (the default): the gateway asks GitHub once a
                                             day for nolune's newest release, and admins see when
                                             there's one
  nolune config set compact-when-idle <minutes|off>
                                             summarize a chat after that many minutes without a
                                             message, so its next reply reads less; off (the
                                             default): only near the model's window, or on request
  nolune key set <anthropic|openai|openrouter|xai|firecrawl> [key]
                                             store an API key (prompts if omitted) after checking
                                             it; OpenAI's runs GPT chats and makes pictures, xAI's
                                             runs Grok, Firecrawl's lifts the web search's daily
                                             limit. Admins can also do this on the web, under
                                             Models & keys
  nolune key rm <anthropic|openai|openrouter|xai|firecrawl>
                                             remove a stored key (the environment's is used, if set)
  nolune env set <NAME> <value>                 extra env var for agent commands (e.g. HASS_TOKEN)
  nolune env rm <NAME> | nolune env list

Custom providers (model servers of your own: Ollama, LM Studio, oMLX, vLLM, llama.cpp...)
  nolune provider add <name> <url> [--api openai|anthropic] [--key K]
                                             add one, through its OpenAI API (the default) or its
                                             Anthropic API, or change the address and key of the
                                             one of that name; nolune asks it for its models first,
                                             and for its key at a terminal when it wants one. The
                                             address is the server's, like http://localhost:11434.
                                             A server that speaks both can be added once for each
  nolune provider rm <name|id>                  presets on it stop working until they're moved
  nolune provider list

Plans (chats on your own subscription instead of an API key)
  claude-plan: a Claude Pro or Max plan, through Claude Code on this computer, signed in to your
  Claude account; Claude Code keeps the sign-in. chatgpt-plan: a ChatGPT Plus or Pro plan, signed
  in with ChatGPT (Sign in with ChatGPT), which nolune keeps in ~/.nolune/chatgpt.json.
  Plan limits assume one person's ordinary use: keep busy automations and subagents on an API key.
  nolune <plan> status                          who it's signed in as (and which Claude Code runs)
  nolune claude-plan setup                      install Claude Code and sign in, where needed,
                                             asking first, in a terminal
  nolune chatgpt-plan setup [--another-account] sign in with ChatGPT in a browser, where needed; from
                                             another device, paste the address it ends on. Signs
                                             in to the account used last unless told otherwise
  nolune chatgpt-plan logout                    sign out; chats on chatgpt-plan presets stop until
                                             someone signs in again
  nolune chatgpt-plan models                    the models the plan offers, for \`nolune preset add\`
  nolune-plan: a subscription to nolune itself, which covers chats, pictures and memory search
  with no keys at all, within its 5-hour and weekly limits. Linked with a code you approve on
  nolune's page, from any device; nolune keeps the link in ~/.nolune/nolune-plan.json.
  nolune nolune-plan setup                      link nolune to your plan, where needed
  nolune nolune-plan logout                     unlink; chats on nolune-plan presets stop until it's
                                             linked again
  nolune nolune-plan models                     the models the plan offers, for \`nolune preset add\`

Users (there's no sign-up page: admins add people here, or on the People page)
  nolune user create <name> <email> [--password P] [--admin]
  nolune user invite [name]                     a link to send, where they make their own account
                                             (once, within 7 days); name: who it's for
  nolune user passwd <name|email> [--password P]
  nolune user admin <name|email> [--off]
  nolune user rm <name|email>
  nolune user list                              name, email, admin, and their picture's
                                             file if they have one

Model presets (shared by all profiles)
  nolune preset add <model> [--provider anthropic|openai|openrouter|xai|claude-plan|chatgpt-plan|nolune-plan|<custom>]
                 [--name N] [--context-window TOKENS]
                                             the provider checks the model id first (anthropic
                                             unless given); OpenAI models other than the
                                             flagships need --context-window. OpenRouter's ids
                                             name their maker (anthropic/claude-sonnet-5), and the
                                             model must be able to call tools. xAI's are Grok's
                                             (grok-4.7), and it lists them with their windows. A
                                             custom provider (by its name) lists its models; its
                                             model must call tools too, which shows at its first
                                             reply, and pictures and PDFs reach it as paths. The
                                             plans check their sign-in instead, and chatgpt-plan
                                             the models the plan offers
  nolune preset edit <name|id> [--provider P] [--model M] [--name N]
                 [--context-window TOKENS|auto]
                                             change what's given; a new model is checked like
                                             add's. Chats already on the preset keep what they had
  nolune preset rm <name|id>
  nolune preset default <name|id>               the model new chats start with
  nolune preset list

${PROFILE_HELP}
  nolune skill new <name> [--description D] [--profile SLUG | --global]
  nolune skill list [--profile SLUG]
  nolune skill enable <name>... [--profile SLUG]
  nolune skill disable <name>... [--profile SLUG]  leave out of the profile's chats

${MCP_HELP}

${TRIGGER_HELP}

${MEMORY_HELP}

${CARD_HELP}

${SOUL_HELP}

${generateHelp()}

${WEB_HELP}

Inside agent commands (NOLUNE_PROFILE is set, so --profile can be left out)
  nolune view <image>...                        show images to the agent: they're attached to the
                                             command's result (HEIC and big photos are converted)

${AGENT_HELP}`;

function positional(args: string[], index: number, name: string): string {
	const value = args[index];
	if (!value) fail(`missing <${name}>. See \`nolune help\`.`);
	return value;
}

/**
 * Checks a pasted key with its provider and saves it. A key the provider rejects isn't saved; one
 * it couldn't be asked about is, with a warning, so setup works while the provider is unreachable.
 */
async function storeApiKey(io: Io, provider: ApiKeyProvider, pasted: string): Promise<void> {
	const { label } = API_KEYS[provider];
	let key: string;
	try {
		key = normalizeApiKey(pasted);
	} catch (err) {
		fail((err as Error).message);
	}
	try {
		const warning = await checkApiKey(provider, key);
		saveApiKey(provider, key);
		io.log(`Saved the ${label} API key.${warning ? ` ${warning}` : ''}`);
	} catch (err) {
		if (!(err instanceof ApiKeyError) || err.reason !== 'unchecked') fail((err as Error).message);
		saveApiKey(provider, key);
		io.log(`Saved the ${label} API key without checking it. ${err.message}`);
	}
}

/**
 * Checks a custom provider's server by asking it for its models, and saves it (adds it, or changes
 * the one of that name): its id and the models it serves. One that wants a key gets asked for it
 * at a terminal; one that can't be reached is saved with a warning, so setup works before the
 * server runs.
 */
async function storeCustomProvider(
	io: Io,
	name: string,
	url: string,
	api: CustomApi | undefined,
	given: string | null
): Promise<{ id: string; models: string[] }> {
	let key = given?.trim() || null;
	const known = findCustomProvider(name);
	if (known && api && api !== known.api) {
		fail(
			`${known.name} speaks ${known.api === 'openai' ? "OpenAI's" : "Anthropic's"} API. Add the other one under another name.`
		);
	}
	let found: Awaited<ReturnType<typeof checkCustomProvider>>;
	try {
		try {
			found = await checkCustomProvider(url, key);
		} catch (err) {
			if (!(err instanceof CustomProviderError) || err.reason !== 'key' || key || !io.stdinIsTTY) {
				throw err;
			}
			key = (await askHidden(io, `Its API key`)) || null;
			if (!key) throw err;
			found = await checkCustomProvider(url, key);
		}
	} catch (err) {
		if (!(err instanceof CustomProviderError) || err.reason !== 'unreachable') {
			fail((err as Error).message);
		}
		const id = saveOrFail(known, name, url, api, key);
		io.log(`Saved ${findCustomProvider(id)?.name} without checking it. ${err.message}`);
		return { id, models: [] };
	}
	const id = saveOrFail(known, name, url, api, key);
	const saved = findCustomProvider(id)!;
	const some = found.models.slice(0, 8).join(', ');
	const serves = found.models.length
		? ` It serves ${some}${found.models.length > 8 ? ` and ${found.models.length - 8} more` : ''}.`
		: '';
	io.log(
		`${known ? 'Changed' : 'Added'} ${saved.name} at ${saved.url}, through its ${saved.api === 'openai' ? 'OpenAI' : 'Anthropic'} API.${serves}${found.warning ? ` ${found.warning}` : ''}`
	);
	return { id, models: found.models };
}

function saveOrFail(
	known: { id: string; name: string } | undefined,
	name: string,
	url: string,
	api: CustomApi | undefined,
	key: string | null
): string {
	try {
		return saveCustomProvider({ id: known?.id, name: known?.name ?? name, api, url, key });
	} catch (err) {
		fail((err as Error).message);
	}
}

/**
 * A preset's provider and model as typed: a custom provider by its name or id, its model kept as
 * `<id>/<model>`. nolune's own providers as they are.
 */
function presetTarget(
	provider: string | undefined,
	model: string
): { provider: string | undefined; model: string } {
	if (!provider || isProvider(provider)) return { provider, model };
	const custom = findCustomProvider(provider);
	if (!custom) {
		const names = listCustomProviders().map((c) => c.name);
		fail(
			`no provider "${provider}". It's anthropic, openai, openrouter, xai, claude-plan, chatgpt-plan, nolune-plan${names.length ? `, or a custom provider: ${names.join(', ')}` : ', or a custom provider added with `nolune provider add <name> <url>`'}.`
		);
	}
	const bare = model.startsWith(`${custom.id}/`) ? model.slice(custom.id.length + 1) : model;
	return { provider: providerFor(custom.api), model: `${custom.id}/${bare}` };
}

/** A preset's model for people: a custom provider's by its name. */
function presetSource(provider: string, model: string): string {
	const on = isCustomProvider(provider) ? splitModel(model) : null;
	const custom = on?.provider ? findCustomProvider(on.provider) : undefined;
	return custom && on ? `${custom.name}/${on.model}` : `${provider}/${model}`;
}

function requireInit(): void {
	if (!configExists()) fail('not set up yet. Run `nolune setup` first.');
}

function resolveProfileSlug(io: Io, flag: string | undefined): string {
	const slug = flag || io.env.NOLUNE_PROFILE;
	if (!slug) fail('which profile? Pass --profile <slug> (or --global for a global skill).');
	if (!getProfileBySlug(slug)) fail(`no profile with slug "${slug}". See \`nolune profile list\`.`);
	return slug;
}

function formatTokens(n: number | null): string {
	if (n == null) return '?';
	return n >= 1_000_000 ? `${n / 1_000_000}M` : n >= 1000 ? `${Math.round(n / 1000)}K` : String(n);
}

function listenAddress() {
	const config = readConfig();
	return {
		host: config.host ?? '127.0.0.1',
		port: config.port ?? DEFAULT_PORT,
		origin: publicOrigin(config)
	};
}

/** macOS keeps background processes out of these folders; elsewhere there's nothing to say. */
function fullDiskAccessHint(): string {
	if (process.platform !== 'darwin') return '';
	if (appManaged()) {
		return '\n\nTo let the agent reach Documents, Desktop, Downloads, Photos and Mail, turn on nolune in System Settings > Privacy & Security > Full Disk Access.';
	}
	return `\n\nTo let the agent reach Documents, Desktop, Downloads, Photos and Mail, give Full Disk Access to
  ${process.execPath}
  in System Settings > Privacy & Security > Full Disk Access (click +, press Cmd+Shift+G, paste the path).
  Note: this applies to every script run with that node binary.`;
}

async function setup(io: Io, args: string[]): Promise<void> {
	const { values } = parseArgs({
		args,
		options: {
			name: { type: 'string' },
			email: { type: 'string' },
			password: { type: 'string' },
			origin: { type: 'string' },
			port: { type: 'string' },
			relay: { type: 'boolean' },
			'relay-name': { type: 'string' },
			url: { type: 'string' },
			api: { type: 'string' }
		},
		allowNegative: true
	});

	const { created } = initConfig();
	getDb();
	installCliShim();
	io.log(created ? `Created ${paths.home}` : `Using ${paths.home}`);

	const admin = listUsers().find((u) => u.isAdmin);
	if (admin) {
		io.log(`Admin account: ${admin.name} <${admin.email}>`);
	} else {
		const name = values.name ?? (await ask(io, 'Your name (the agent sees it on your messages)'));
		const email = values.email ?? (await ask(io, 'Your email (to sign in)'));
		const password = values.password ?? generatePassword();
		await createUser({ name, email, password, isAdmin: true });
		io.log(`Created your account. Password: ${values.password ? '(as given)' : password}`);
	}

	const current = listenAddress();
	const port = values.port ? Number(values.port) : current.port;
	updateConfig((c) => {
		c.port = port;
	});
	const relay = values.origin === undefined && (await setupRelay(io, values));
	if (!relay) {
		const origin =
			values.origin ??
			(await ask(
				io,
				'Public URL people will open (leave as is for this computer only)',
				readConfig().origin ?? `http://localhost:${port}`
			));
		updateConfig((c) => {
			c.origin = origin;
		});
	}
	const origin = publicOrigin(readConfig());

	const reach = relay
		? `Family members open that address on any device, at home or away, while the gateway runs. The
relay passes traffic between their browsers and this computer, and could see it, as any tunnel
could: \`nolune relay disable\` stops using it.`
		: `The gateway listens on http://${current.host}:${port}. To reach it from outside your home, run
\`nolune relay enable\` for an address through nolune's relay, or point a tunnel of your own at
it (Tailscale Funnel, Cloudflare Tunnel, or a VPS) and set its URL with
\`nolune config set origin https://...\`. Notifications on the nolune app for iPhone only come
through the relay.`;

	io.log(`
Done. Next:
  nolune service install        run the gateway in the background (or \`nolune start\` to try it)
  nolune user invite Anna       a link for a family member (or People, in the web UI)
  open ${origin}

Chats need a model: sign in and make a profile, and its welcome asks for one (a key or a plan,
then the model). Or add one under Models & keys, or with \`nolune key set\` and \`nolune preset add\`.

${reach}${fullDiskAccessHint()}`);
}

/**
 * Setup's question of how the family reaches nolune: through the relay unless they'd rather not
 * (`--relay` / `--no-relay` answer it; without a terminal and without either, it's no). Returns
 * whether nolune has a relay address, which it keeps once it has one. One it can't get leaves the
 * choice to the origin.
 */
async function setupRelay(
	io: Io,
	values: { relay?: boolean; 'relay-name'?: string }
): Promise<boolean> {
	const existing = readConfig().relay;
	if (existing) {
		io.log(`Address (through nolune's relay): ${existing.url}`);
		return true;
	}
	let wanted = values.relay;
	if (wanted === undefined && io.stdinIsTTY) {
		io.log(`
How will your family open nolune? nolune's relay gives it an address like
https://smiths.nolune.family that works on any device, at home or away, with no tunnel or
port forwarding. It passes their traffic to this computer, and could see it, as any tunnel
could. Or keep nolune to this computer, or give a URL of your own: then the nolune app for
iPhone gets no notifications, which only come through the relay.`);
		wanted = /^y/i.test(await ask(io, 'Use the relay? (y/n)', 'y'));
	}
	if (!wanted) return false;
	let name = values['relay-name'];
	for (let tries = 0; ; tries++) {
		if (name === undefined && io.stdinIsTTY) {
			name = await ask(
				io,
				'A name for the address, like smiths (letters, digits and dashes; empty for a random one)'
			);
		}
		try {
			const { relay } = await enableRelay(io, { name });
			io.log(`Address: ${relay.url}`);
			return true;
		} catch (err) {
			io.error(`nolune: ${(err as Error).message}`);
			// Another name may do; the relay being out of reach won't change by asking again.
			if (!io.stdinIsTTY || tries >= 2 || err instanceof RelayUnreachable) {
				io.log('Leaving the relay off for now: `nolune relay enable` turns it on later.');
				return false;
			}
			name = undefined;
		}
	}
}

/** Runs the gateway in this process, so it only makes sense in a process of its own. */
async function start(io: Io): Promise<void> {
	requireInit();
	if (!existsSync(paths.server)) {
		fail(`no server build at ${paths.server}. In a source checkout, run \`pnpm build\` first.`);
	}
	const { host, port, origin } = listenAddress();
	// Environment variables win over config.json, as adapter-node expects.
	process.env.HOST ??= host;
	process.env.PORT ??= String(port);
	process.env.ORIGIN ??= origin;
	const { relay } = readConfig();
	// adapter-node refuses bodies over 512 KB; attachments go up to MAX_MEDIA_BYTES. Every route
	// except the upload one keeps a 1 MB limit (src/hooks.server.ts).
	process.env.BODY_SIZE_LIMIT ??= String(MAX_MEDIA_BYTES + 1024 * 1024);
	io.log(
		`nolune gateway: ${process.env.ORIGIN} (listening on ${process.env.HOST}:${process.env.PORT})`
	);
	// `pnpm dev` loads this file through Vite (src/hooks.server.ts), which can't follow a runtime
	// path: the built server is loaded by Node, as is.
	await import(/* @vite-ignore */ pathToFileURL(paths.server).href);
	if (relay) {
		// The relay's requests come in as anyone's would, to the address the server listens on.
		const listening = process.env.HOST;
		const link = connectRelay(
			relay,
			{
				host: !listening || listening === '0.0.0.0' || listening === '::' ? '127.0.0.1' : listening,
				port: Number(process.env.PORT)
			},
			(message) => io.log(`[nolune] relay: ${message}`)
		);
		// Before adapter-node's shutdown, which waits for open connections, like the relay's
		// event streams, to end.
		for (const signal of ['SIGINT', 'SIGTERM'] as const) process.once(signal, () => link.close());
	}
}

async function service(io: Io, action: string | undefined, args: string[]): Promise<void> {
	switch (action) {
		case 'install': {
			if (appManaged()) fail('the nolune app runs the gateway while it is open.');
			requireInit();
			if (!existsSync(paths.server))
				fail('no server build. In a source checkout, run `pnpm build` first.');
			if (args.includes('--dry-run')) {
				io.log(renderServiceFile());
				return;
			}
			const { file, atBoot } = await installService();
			const { origin } = listenAddress();
			// A Linux user whose services don't linger has them stopped at their last logout.
			const linger =
				atBoot || process.platform !== 'linux'
					? ''
					: `\n\nIt stops when you log out. To keep it running whenever this computer is on, run
  sudo loginctl enable-linger ${userInfo().username}`;
			io.log(`Installed ${file}
The gateway starts now and ${atBoot ? 'whenever this computer starts' : 'at every login'}: ${origin}
Logs: ${logFile}
It runs with ${process.execPath}; run \`nolune service install\` again after switching Node versions.${fullDiskAccessHint()}${linger}`);
			return;
		}
		case 'uninstall':
			if (appManaged()) {
				fail(
					'the nolune app runs the gateway while it is open. Quit it from the menu bar, and turn it off under System Settings > General > Login Items so it stays closed.'
				);
			}
			uninstallService();
			io.log('Removed the background service.');
			return;
		case 'restart':
			restartService();
			io.log('Restarted.');
			return;
		case 'status': {
			const status = serviceStatus();
			if (appManaged() && !status.loaded) io.log('Not running. Open the nolune app.');
			else if (!status.installed) io.log('Not installed. Run `nolune service install`.');
			else if (!status.loaded) io.log('Installed but not loaded. Run `nolune service install`.');
			else
				io.log(
					status.pid
						? `Running (pid ${status.pid}).`
						: 'Loaded, not running. See `nolune service logs`.'
				);
			return;
		}
		case 'logs': {
			if (!existsSync(logFile)) fail(`no log yet at ${logFile}`);
			const follow = args.includes('-f') || args.includes('--follow');
			spawn('tail', ['-n', '100', ...(follow ? ['-f'] : []), logFile], { stdio: 'inherit' });
			return;
		}
		default:
			fail('usage: nolune service install|uninstall|restart|status|logs [-f]');
	}
}

/**
 * Runs `nolune` with these arguments and returns its exit code. Everything it reads and writes
 * besides its arguments and nolune's own files goes through `io`. An error ends it with its message
 * and exit code 1.
 */
export async function runCli(argv: string[], io: Io): Promise<number> {
	try {
		return (await command(io, argv)) ?? 0;
	} catch (err) {
		io.error(`nolune: ${err instanceof Error ? err.message : String(err)}`);
		return 1;
	}
}

async function command(io: Io, argv: string[]): Promise<number | void> {
	const [group, action, ...rest] = argv;

	switch (group) {
		case undefined:
		case 'help':
		case '--help':
		case '-h':
			io.log(help());
			return;

		case 'setup':
			return setup(io, argv.slice(1));

		case 'start':
			return start(io);

		case 'service':
			return service(io, action, rest);

		case 'relay':
			requireInit();
			return relayCommand(io, action, rest);

		case 'init': {
			// Non-interactive part of `nolune setup`, kept for scripts.
			const { created } = initConfig();
			getDb();
			const shim = installCliShim();
			io.log(created ? `Created ${paths.config}` : `Config already exists: ${paths.config}`);
			io.log(`Database ready: ${paths.db}`);
			io.log(`Agent shim: ${shim}`);
			return;
		}

		case 'config': {
			requireInit();
			if (action === undefined) {
				const config = readConfig();
				const { host, port, origin } = listenAddress();
				const row = (name: string, value: string) => io.log(`${name.padEnd(13)} ${value}`);
				row('home', paths.home);
				row('listen', `http://${host}:${port}`);
				row('origin', origin);
				row(
					'relay',
					config.relay
						? `on, through ${config.relay.server} (nolune relay status)`
						: 'off (nolune relay enable gives nolune an address that works from anywhere)'
				);
				for (const key of apiKeyStatuses()) {
					const where =
						key.source === 'config' ? 'key set' : key.source === 'env' ? `key from ${key.env}` : '';
					const shown = where
						? `${where}${key.hint ? ` (…${key.hint})` : ''}`
						: key.optional
							? `no key: the free tier, limited per day (nolune key set ${key.provider})`
							: `no key (nolune key set ${key.provider})`;
					row(key.provider, shown);
				}
				const customs = listCustomProviders();
				row(
					'custom',
					customs.length
						? customs
								.map((c) => `${c.name} ${c.url} (${c.api}${c.hasKey ? ', with a key' : ''})`)
								.join(', ')
						: 'none (nolune provider add <name> <url>)'
				);
				const claude = claudeExecutable();
				row(
					'claude',
					claude
						? `Claude Code at ${claude} (nolune claude-plan status checks its sign-in)`
						: 'no Claude Code found (nolune claude-plan setup installs it)'
				);
				const chatgpt = await chatGptPlanStatus();
				row(
					'chatgpt',
					chatgpt.signedIn
						? `${chatgpt.signedIn} (nolune chatgpt-plan status checks it)`
						: 'not signed in (nolune chatgpt-plan setup signs in)'
				);
				const noluneLink = await nolunePlanStatus();
				row(
					'nolune plan',
					noluneLink.signedIn
						? `${noluneLink.signedIn} (nolune nolune-plan status checks it)`
						: 'not linked (nolune nolune-plan setup links it)'
				);
				const images = imageGenerationStatus();
				row('images', `${images.model}${images.problem ? ` (${images.problem})` : ''}`);
				row('embeddings', embeddingStatus());
				row('commands', describeCommandSafety());
				row('quiet chats', describeIdleCompaction());
				row('env', Object.keys(config.commandEnv ?? {}).join(', ') || '-');
				row('version', describeUpdates());
				return;
			}
			if (action !== 'set') {
				fail(
					'usage: nolune config [set <host|port|origin|image-model|embeddings|claude-path|command-mode|safety-model|update-check|compact-when-idle> <value>]'
				);
			}
			const key = positional(
				rest,
				0,
				'host|port|origin|image-model|embeddings|claude-path|command-mode|safety-model|update-check|compact-when-idle'
			);
			if (key === 'command-mode' || key === 'safety-model') {
				// Auto mode guards against the agent itself, so it can't be the one to turn it off.
				if (io.env.NOLUNE_CONVERSATION_ID) {
					fail(
						"the agent can't change how its own commands are checked. Someone can, at this computer's terminal or on the Models & keys page."
					);
				}
				if (key === 'command-mode') {
					const mode = positional(rest, 1, 'auto|unrestricted');
					if (!isCommandMode(mode)) fail('command-mode is auto or unrestricted');
					saveCommandMode(mode);
				} else {
					const which = positional(rest, 1, 'preset|chat');
					let id: string | null = null;
					if (which !== 'chat') {
						try {
							id = findPreset(which).id;
						} catch (err) {
							fail(`${(err as Error).message}. \`nolune preset list\` shows them.`);
						}
					}
					saveSafetyPreset(id);
				}
				io.log(`Commands: ${describeCommandSafety()}.`);
				return;
			}
			if (key === 'update-check') {
				const on = positional(rest, 1, 'on|off');
				if (on !== 'on' && on !== 'off') fail('update-check is on or off');
				updateConfig((c) => {
					if (on === 'on') delete c.updateCheck;
					else c.updateCheck = false;
				});
				// What GitHub said last would go stale: the app's menu reads it too.
				if (on === 'off') forgetRelease();
				io.log(
					on === 'on'
						? 'nolune will look for a new release within the hour, then once a day.'
						: 'nolune won’t look for new releases.'
				);
				return;
			}
			if (key === 'compact-when-idle') {
				const value = positional(rest, 1, 'minutes|off');
				try {
					saveIdleCompaction(value === 'off' ? null : Number(value));
				} catch (err) {
					fail(`${(err as Error).message}, or off.`);
				}
				idleCompactionChanged();
				io.log(`Quiet chats: ${describeIdleCompaction()}.`);
				return;
			}
			if (key === 'embeddings') {
				let setting: ReturnType<typeof parseEmbeddingSetting>;
				try {
					setting = parseEmbeddingSetting(positional(rest, 1, 'auto|off|provider/model'));
				} catch (err) {
					fail((err as Error).message);
				}
				saveEmbeddingSetting(setting);
				const problem = await embeddingProblem();
				io.log(
					`Memory search by meaning: ${embeddingStatus()}.${problem ? ` It didn't answer: ${problem}` : ''}`
				);
				return;
			}
			const value = positional(rest, 1, 'value');
			updateConfig((c) => {
				if (key === 'port') {
					const port = Number(value);
					if (!Number.isInteger(port) || port <= 0) fail('port must be a number');
					c.port = port;
				} else if (key === 'host') c.host = value;
				else if (key === 'origin') c.origin = value.replace(/\/+$/, '');
				else if (key === 'image-model') {
					const current = parseImageModel(c.imageModel || configuredImageModel());
					const { provider, model } = parseImageModel(value, current.provider);
					c.imageModel = `${provider}/${model}`;
				} else if (key === 'claude-path') c.claudePath = value;
				else
					fail(
						'you can set host, port, origin, image-model, embeddings, claude-path, command-mode, safety-model, update-check or compact-when-idle'
					);
			});
			if (key === 'claude-path') {
				await requireClaudePlan(io);
				return;
			}
			if (key === 'image-model') {
				const { model, problem } = imageGenerationStatus();
				io.log(`Pictures are now made with ${model}.${problem ? ` ${problem}` : ''}`);
				return;
			}
			const relayNote =
				key === 'origin' && readConfig().relay
					? ` While the relay is on, nolune's address stays ${readConfig().relay?.url}; \`nolune relay disable\` switches to this one.`
					: '';
			io.log(`Set ${key}. Run \`nolune service restart\` if the service is running.${relayNote}`);
			return;
		}

		case 'key': {
			requireInit();
			const provider = rest[0] ?? '';
			const names = Object.keys(API_KEYS).join('|');
			if ((action !== 'set' && action !== 'rm') || !isApiKeyProvider(provider)) {
				fail(
					`usage: nolune key set <${names}> [key] | nolune key rm <${names}> (your own servers: nolune provider add)`
				);
			}
			const { label, env } = API_KEYS[provider];
			if (action === 'rm') {
				removeApiKey(provider);
				const fallback = io.env[env] ? ` nolune uses ${env} from the environment now.` : '';
				io.log(`Removed the ${label} API key.${fallback}`);
				return;
			}
			const key = rest[1] || (await askHidden(io, `${label} API key`));
			if (!key) fail('no key given');
			await storeApiKey(io, provider, key);
			return;
		}

		case 'provider': {
			requireInit();
			const { values, positionals } = parseArgs({
				args: rest,
				allowPositionals: true,
				options: { key: { type: 'string' }, api: { type: 'string' } }
			});
			if (action === 'add') {
				const name = positional(positionals, 0, 'name');
				const url = positional(positionals, 1, 'url');
				const api = values.api;
				if (api !== undefined && api !== 'openai' && api !== 'anthropic') {
					fail('--api is openai or anthropic');
				}
				await storeCustomProvider(io, name, url, api, values.key ?? null);
			} else if (action === 'rm') {
				const which = positional(positionals, 0, 'name|id');
				const custom = findCustomProvider(which);
				if (!custom) fail(`no custom provider "${which}". See \`nolune provider list\`.`);
				const on = listPresets().filter(
					(p) => isCustomProvider(p.provider) && splitModel(p.model).provider === custom.id
				);
				removeCustomProvider(custom.id);
				io.log(
					`Removed ${custom.name}.${on.length ? ` Chats on ${on.map((p) => `"${p.name}"`).join(', ')} stop working until they're moved to another model.` : ''}`
				);
			} else if (action === 'list') {
				for (const c of listCustomProviders()) {
					const key = c.hasKey ? `key${c.hint ? ` …${c.hint}` : ''}` : 'no key';
					io.log(`${c.name}\t${c.id}\t${c.api}\t${c.url}\t${key}`);
				}
			} else
				fail(
					'usage: nolune provider add <name> <url> [--api openai|anthropic] [--key K] | nolune provider rm <name|id> | nolune provider list'
				);
			return;
		}

		case 'claude-plan':
		case 'chatgpt-plan':
		case 'nolune-plan':
			return planCommand(io, group, action, rest);

		case 'env': {
			requireInit();
			if (action === 'list') {
				for (const name of Object.keys(readConfig().commandEnv ?? {})) io.log(name);
			} else if (action === 'set') {
				const name = positional(rest, 0, 'NAME');
				const value = rest[1] ?? (await askHidden(io, name));
				if (!value) fail('no value given');
				updateConfig((c) => {
					c.commandEnv = { ...c.commandEnv, [name]: value };
				});
				io.log(`Set ${name} for agent commands.`);
			} else if (action === 'rm') {
				const name = positional(rest, 0, 'NAME');
				updateConfig((c) => {
					if (c.commandEnv) delete c.commandEnv[name];
				});
				io.log(`Removed ${name}.`);
			} else fail('usage: nolune env set|rm|list');
			return;
		}

		case 'user': {
			requireInit();
			const { values, positionals } = parseArgs({
				args: rest,
				allowPositionals: true,
				options: {
					password: { type: 'string' },
					admin: { type: 'boolean' },
					off: { type: 'boolean' }
				}
			});
			if (action === 'create') {
				const name = positional(positionals, 0, 'name');
				const email = positional(positionals, 1, 'email');
				const password = values.password ?? generatePassword();
				await createUser({ name, email, password, isAdmin: values.admin });
				io.log(`Created ${name} <${email}>${values.admin ? ' (admin)' : ''}.`);
				if (!values.password) io.log(`Password: ${password}`);
			} else if (action === 'passwd') {
				const who = positional(positionals, 0, 'name|email');
				const password = values.password ?? generatePassword();
				await setPassword(who, password);
				io.log(`Password changed for ${who}.`);
				if (!values.password) io.log(`Password: ${password}`);
			} else if (action === 'admin') {
				const who = positional(positionals, 0, 'name|email');
				setAdmin(who, !values.off);
				io.log(`${who} is ${values.off ? 'no longer' : 'now'} an admin.`);
			} else if (action === 'rm') {
				const who = positional(positionals, 0, 'name|email');
				deleteUser(who);
				io.log(`Deleted ${who}.`);
			} else if (action === 'invite') {
				const { token } = createInvite({ name: positionals[0] });
				io.log(`${publicOrigin(readConfig())}/invite/${token}`);
				io.log(`Works once, within ${INVITE_DAYS} days. Take it back on the People page.`);
			} else if (action === 'list') {
				for (const u of listUsers()) {
					// Their picture's file last, for the macOS app's menu.
					const picture = u.picture ? userPictureFile(u.picture) : null;
					const fields = [u.name, u.email, u.isAdmin && 'admin', picture?.path];
					io.log(fields.filter(Boolean).join('\t'));
				}
			} else fail('usage: nolune user create|invite|passwd|admin|rm|list');
			return;
		}

		case 'preset': {
			requireInit();
			const { values, positionals } = parseArgs({
				args: rest,
				allowPositionals: true,
				options: {
					provider: { type: 'string' },
					model: { type: 'string' },
					name: { type: 'string' },
					'context-window': { type: 'string' }
				}
			});
			if (action === 'add') {
				const { provider, model } = presetTarget(
					values.provider,
					positional(positionals, 0, 'model')
				);
				const cw = values['context-window'];
				const preset = await addPreset({
					provider,
					model,
					name: values.name,
					contextWindow: cw ? Number(cw) : null
				});
				io.log(`Added "${preset.name}" (context ${formatTokens(effectiveContextWindow(preset))}).`);
			} else if (action === 'edit') {
				const cw = values['context-window'];
				const which = positional(positionals, 0, 'name|id');
				let { provider, model } = { provider: values.provider, model: values.model };
				if (provider && !isProvider(provider)) {
					// A custom provider keeps the model's id there, unless another is given.
					const old = listPresets().find((p) => p.id === which || p.name === which);
					const id =
						old && isCustomProvider(old.provider) ? splitModel(old.model).model : old?.model;
					({ provider, model } = presetTarget(provider, model ?? id ?? ''));
				}
				const preset = await editPreset(which, {
					provider,
					model,
					name: values.name,
					contextWindow: cw === undefined ? undefined : cw === 'auto' ? null : Number(cw)
				});
				io.log(
					`Saved "${preset.name}" (${presetSource(preset.provider, preset.model)}, context ${formatTokens(effectiveContextWindow(preset))}). Chats already on it keep what they had.`
				);
			} else if (action === 'rm') {
				removePreset(positional(positionals, 0, 'name|id'));
				io.log('Removed. Existing conversations keep working.');
			} else if (action === 'default') {
				const preset = setDefaultPreset(positional(positionals, 0, 'name|id'));
				io.log(`"${preset.name}" is now the default. New chats start with it.`);
			} else if (action === 'list') {
				const defaultId = getDefaultPreset()?.id;
				for (const p of listPresets()) {
					const override = p.contextWindow ? ' (override)' : '';
					const isDefault = p.id === defaultId ? '\tdefault' : '';
					io.log(
						`${p.name}\t${presetSource(p.provider, p.model)}\tcontext ${formatTokens(effectiveContextWindow(p))}${override}\t${p.id}${isDefault}`
					);
				}
			} else fail('usage: nolune preset add|edit|rm|default|list');
			return;
		}

		case 'profile':
			requireInit();
			return profileCommand(io, action, rest);

		case 'skill': {
			const { values, positionals } = parseArgs({
				args: rest,
				allowPositionals: true,
				options: {
					description: { type: 'string', short: 'd' },
					profile: { type: 'string' },
					global: { type: 'boolean' }
				}
			});
			if (action === 'new') {
				const name = positional(positionals, 0, 'name');
				const slug = values.global ? null : resolveProfileSlug(io, values.profile);
				const dir = slug ? profileSkillsDir(slug) : paths.globalSkills;
				const location = createSkill(dir, name, values.description ?? '');
				io.log(`Created ${location}`);
				if (slug && getProfileBySlug(slug)?.disabledSkills.includes(name)) {
					io.log(
						`"${name}" is turned off in this profile, so its chats won't list it. Turn it on with \`nolune skill enable ${name}\`.`
					);
				}
			} else if (action === 'list') {
				const slug = values.profile || io.env.NOLUNE_PROFILE;
				const profile = slug ? getProfileBySlug(slug) : undefined;
				const { skills, warnings } = listProfileSkills(
					slug ? profileSkillsDir(slug) : '/nonexistent',
					profile?.disabledSkills ?? []
				);
				for (const s of skills) {
					const state = profile ? `${s.enabled ? 'on' : 'off'}\t` : '';
					io.log(
						`${s.name}\t${state}${s.scope}\t~${s.tokens} tokens\t${s.location}\n  ${s.description}`
					);
				}
				for (const w of warnings) io.error(`warning: ${w}`);
				if (!skills.length) io.log('No skills.');
				else if (profile) {
					const on = skills.filter((s) => s.enabled);
					const tokens = on.reduce((n, s) => n + s.tokens, 0);
					io.log(
						`\n${on.length} of ${skills.length} on in ${profile.name}, about ${formatTokens(tokens)} tokens in every new chat.`
					);
				}
			} else if (action === 'enable' || action === 'disable') {
				if (values.global) fail('skills are turned on and off per profile. Pass --profile <slug>.');
				const profile = getProfileBySlug(resolveProfileSlug(io, values.profile))!;
				if (!positionals.length) fail('missing <name>. See `nolune help`.');
				const known = new Set(scanSkills(profileSkillsDir(profile.slug)).skills.map((s) => s.name));
				const unknown = positionals.filter((name) => !known.has(name));
				if (unknown.length)
					fail(`no skill named ${unknown.join(', ')}. See \`nolune skill list\`.`);
				setSkillsEnabled(profile.id, positionals, action === 'enable');
				io.log(
					`${positionals.join(', ')}: ${action === 'enable' ? 'on' : 'off'} for new chats in ${profile.name}.`
				);
			} else fail('usage: nolune skill new|list|enable|disable');
			return;
		}

		case 'mcp':
			requireInit();
			return mcpCommand(io, action, rest);

		case 'trigger':
			requireInit();
			return triggerCommand(io, action, rest);

		case 'memory':
			requireInit();
			return memoryCommand(io, argv.slice(1));

		case 'card':
			requireInit();
			return cardCommand(io, argv.slice(1));

		case 'soul':
			requireInit();
			return soulCommand(io, argv.slice(1));

		case 'wake':
			requireInit();
			return wakeCommand(io, argv.slice(1));

		case 'agent':
			requireInit();
			return agentCommand(io, action, rest);

		case 'generate':
			return generateCommand(io, action, rest);

		case 'web':
			return webCommand(io, action, rest);

		case 'view': {
			const dir = io.env.NOLUNE_VIEW_DIR;
			if (!dir)
				fail("`nolune view` only works in the agent's commands: it shows images to the agent.");
			const files = argv.slice(1);
			if (!files.length) fail('usage: nolune view <image>...');
			let failed = false;
			for (const [i, file] of files.entries()) {
				try {
					io.log(await viewImage(file, dir, io.cwd));
				} catch (err) {
					failed = true;
					const message = err instanceof Error ? err.message : String(err);
					io.error(`nolune: can't show ${file}: ${message.replace(/\.+$/, '')}.`);
					if (err instanceof ViewLimitError) {
						const rest = files.slice(i + 1);
						if (rest.length) io.error(`nolune: not shown either: ${rest.join(' ')}`);
						break;
					}
				}
			}
			return failed ? 1 : 0;
		}

		default:
			fail(`unknown command "${group}". See \`nolune help\`.`);
	}
}

/** After how many quiet minutes chats are summarized, in words. */
function describeIdleCompaction(): string {
	const minutes = idleCompactionMinutes();
	return minutes
		? `summarized after ${minutes} minute${minutes === 1 ? '' : 's'} without a message`
		: 'summarized only near the window, or on request';
}
