import { existsSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import {
	API_KEYS,
	ApiKeyError,
	CUSTOM_OPENAI_ENV,
	CustomOpenaiError,
	DEFAULT_IMAGE_MODEL,
	DEFAULT_PORT,
	MAX_MEDIA_BYTES,
	addPreset,
	editPreset,
	apiKeyStatuses,
	checkApiKey,
	checkCustomOpenai,
	claudeExecutable,
	codexExecutable,
	configExists,
	customOpenaiStatus,
	createSkill,
	createUser,
	deleteUser,
	effectiveContextWindow,
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
	isProvider,
	listPresets,
	listProfileSkills,
	listUsers,
	normalizeApiKey,
	parseImageModel,
	paths,
	profileSkillsDir,
	readConfig,
	removeApiKey,
	removeCustomOpenai,
	removePreset,
	isPlan,
	saveApiKey,
	saveCustomOpenai,
	scanSkills,
	setAdmin,
	setDefaultPreset,
	setPassword,
	setSkillsEnabled,
	updateConfig,
	viewImage,
	ViewLimitError,
	type ApiKeyProvider,
	type Provider
} from '@btw/core';
import { AGENT_HELP, agentCommand } from './agent.ts';
import { generateCommand, generateHelp } from './generate.ts';
import { ask, askHidden } from './input.ts';
import { fail, type Io } from './io.ts';
import { planCommand, requireChatGptPlan, requireClaudePlan, setUpPlan } from './plans.ts';
import { MEMORY_HELP, memoryCommand } from './memory.ts';
import { PROFILE_HELP, profileCommand } from './profile.ts';
import { SOUL_HELP, soulCommand } from './soul.ts';
import { TRIGGER_HELP, triggerCommand, wakeCommand } from './triggers.ts';
import {
	installService,
	logFile,
	renderPlist,
	restartService,
	serviceStatus,
	uninstallService
} from './service.ts';

/** Built when shown, like generateHelp(): the gateway serves it for as long as it runs. */
const help = () => `btw - a family agent that runs on this computer

Getting started
  btw setup [--provider anthropic|openai|openrouter|custom-openai|claude-plan|chatgpt-plan]
          [--url ADDRESS]                    interactive first-time setup (key, your account, model);
                                             chats run on Claude unless you pick another: openrouter
                                             runs any model OpenRouter serves with one key,
                                             custom-openai the models of a server at --url that
                                             speaks OpenAI's API (Ollama, LM Studio, vLLM...), and a
                                             plan (see Plans below) is signed in to instead
  btw start                                  run the gateway in the foreground
  btw service install|uninstall|restart|status|logs [-f]
                                             run it in the background at login (macOS)

Settings (${paths.home})
  btw config                                 show address, port and what's configured
  btw config set <host|port|origin> <value>  origin = the public URL people open
  btw config set image-model <provider/model>  for pictures, e.g. openai/gpt-image-2.5-flare
  btw config set embeddings <auto|off|provider/model>
                                             what memory search finds meaning with: auto uses the
                                             OpenAI key, else OpenRouter's; the provider is openai,
                                             openrouter or custom-openai (your server's model, like
                                             custom-openai/nomic-embed-text)
  btw config set claude-path <path>          the Claude Code that claude-plan chats run, and the
  btw config set codex-path <path>           Codex that chatgpt-plan chats run (found on the PATH
                                             and in their usual folders otherwise)
  btw key set <anthropic|openai|openrouter> [key]
                                             store an API key (prompts if omitted) after checking
                                             it; OpenAI's runs GPT chats and makes pictures. Admins
                                             can also do this on the web, under Models & keys
  btw key rm <anthropic|openai|openrouter>   remove a stored key (the environment's is used, if set)
  btw key set custom-openai <url> [key]      the Custom OpenAI server: any server that speaks
                                             OpenAI's API, like Ollama (http://localhost:11434/v1)
                                             or LM Studio (http://localhost:1234/v1), and its key if
                                             it wants one; btw asks it for its models first
  btw key rm custom-openai                   forget it (${CUSTOM_OPENAI_ENV.url} is used, if set)
  btw env set <NAME> <value>                 extra env var for agent commands (e.g. FIRECRAWL_API_KEY)
  btw env rm <NAME> | btw env list

Plans (chats on your own subscription instead of an API key)
  claude-plan: a Claude Pro or Max plan, through Claude Code on this computer, signed in to your
  Claude account. chatgpt-plan: a ChatGPT Plus, Pro or Business plan, through OpenAI's Codex on
  this computer, signed in with ChatGPT. btw never sees either sign-in: the agent keeps it.
  Plan limits assume one person's ordinary use: keep busy automations and subagents on an API key.
  btw <plan> status                          which Claude Code or Codex btw runs, and who it's
                                             signed in as
  btw <plan> setup                           install it and sign in, where needed. Installing asks
                                             first, in a terminal; claude-plan signs in there too,
                                             chatgpt-plan with a link and a code for any device
  btw chatgpt-plan logout                    sign Codex out; chats on chatgpt-plan presets stop
                                             until someone signs in again
  btw chatgpt-plan models                    the models the plan offers, for \`btw preset add\`

Users (web sign-up is disabled; this is the only way to add people)
  btw user create <name> <email> [--password P] [--admin]
  btw user passwd <name|email> [--password P]
  btw user admin <name|email> [--off]
  btw user rm <name|email>
  btw user list

Model presets (shared by all profiles)
  btw preset add <model> [--provider anthropic|openai|openrouter|custom-openai|claude-plan|
                 chatgpt-plan] [--name N] [--context-window TOKENS]
                                             the provider checks the model id first (anthropic
                                             unless given); OpenAI models other than the
                                             flagships need --context-window. OpenRouter's ids
                                             name their maker (anthropic/claude-sonnet-5), and the
                                             model must be able to call tools. A custom-openai
                                             model must call tools too, which shows at its first
                                             reply; pictures and PDFs reach it as paths. The plans
                                             check their agent's sign-in instead, and chatgpt-plan
                                             the models Codex offers
  btw preset edit <name|id> [--provider P] [--model M] [--name N] [--context-window TOKENS|auto]
                                             change what's given; a new model is checked like
                                             add's. Chats already on the preset keep what they had
  btw preset rm <name|id>
  btw preset default <name|id>               the model new chats start with
  btw preset list

${PROFILE_HELP}
  btw skill new <name> [--description D] [--profile SLUG | --global]
  btw skill list [--profile SLUG]
  btw skill enable <name>... [--profile SLUG]
  btw skill disable <name>... [--profile SLUG]  leave out of the profile's new chats

${TRIGGER_HELP}

${MEMORY_HELP}

${SOUL_HELP}

${generateHelp()}

Inside agent commands (BTW_PROFILE is set, so --profile can be left out)
  btw view <image>...                        show images to the agent: they're attached to the
                                             command's result (HEIC and big photos are converted)

${AGENT_HELP}`;

/** What `btw setup` suggests for each provider's first preset, and where its keys are made. */
const SETUP: Record<Provider, { model: string; keys: string }> = {
	anthropic: { model: 'claude-opus-5-5', keys: 'console.anthropic.com > API keys' },
	openai: { model: 'gpt-6-astra', keys: 'platform.openai.com > API keys' },
	openrouter: { model: 'anthropic/claude-opus-5.5', keys: 'openrouter.ai > Settings > API Keys' },
	// The first model the server lists.
	'custom-openai': { model: '', keys: '' },
	'claude-plan': { model: 'claude-opus-5-5', keys: '' },
	'chatgpt-plan': { model: 'gpt-6-astra', keys: '' }
};

function positional(args: string[], index: number, name: string): string {
	const value = args[index];
	if (!value) fail(`missing <${name}>. See \`btw help\`.`);
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
 * Checks the Custom OpenAI server by asking it for its models, and saves it: the models it serves.
 * One that wants a key gets asked for it at a terminal; one that can't be reached is saved with a
 * warning, so setup works before the server runs.
 */
async function storeCustomOpenai(io: Io, url: string, given: string | null): Promise<string[]> {
	let key = given?.trim() || null;
	let found: Awaited<ReturnType<typeof checkCustomOpenai>>;
	try {
		try {
			found = await checkCustomOpenai(url, key);
		} catch (err) {
			if (!(err instanceof CustomOpenaiError) || err.reason !== 'key' || key || !io.stdinIsTTY) {
				throw err;
			}
			key = (await askHidden(io, `Its API key`)) || null;
			if (!key) throw err;
			found = await checkCustomOpenai(url, key);
		}
	} catch (err) {
		if (!(err instanceof CustomOpenaiError) || err.reason !== 'unreachable') {
			fail((err as Error).message);
		}
		saveCustomOpenai(url, key);
		io.log(`Saved the Custom OpenAI server without checking it. ${err.message}`);
		return [];
	}
	saveCustomOpenai(url, key);
	const { url: saved } = customOpenaiStatus();
	const some = found.models.slice(0, 8).join(', ');
	const serves = found.models.length
		? ` It serves ${some}${found.models.length > 8 ? ` and ${found.models.length - 8} more` : ''}.`
		: '';
	io.log(
		`Saved the Custom OpenAI server at ${saved}.${serves}${found.warning ? ` ${found.warning}` : ''}`
	);
	return found.models;
}

function requireInit(): void {
	if (!configExists()) fail('not set up yet. Run `btw setup` first.');
}

function resolveProfileSlug(io: Io, flag: string | undefined): string {
	const slug = flag || io.env.BTW_PROFILE;
	if (!slug) fail('which profile? Pass --profile <slug> (or --global for a global skill).');
	if (!getProfileBySlug(slug)) fail(`no profile with slug "${slug}". See \`btw profile list\`.`);
	return slug;
}

function formatTokens(n: number | null): string {
	if (n == null) return '?';
	return n >= 1_000_000 ? `${n / 1_000_000}M` : n >= 1000 ? `${Math.round(n / 1000)}K` : String(n);
}

function listenAddress() {
	const config = readConfig();
	const port = config.port ?? DEFAULT_PORT;
	return {
		host: config.host ?? '127.0.0.1',
		port,
		origin: config.origin ?? `http://localhost:${port}`
	};
}

function fullDiskAccessHint(): string {
	return `To let the agent reach Documents, Desktop, Downloads, Photos and Mail, give Full Disk Access to
  ${process.execPath}
  in System Settings > Privacy & Security > Full Disk Access (click +, press Cmd+Shift+G, paste the path).
  Note: this applies to every script run with that node binary.`;
}

async function setup(io: Io, args: string[]): Promise<void> {
	const { values } = parseArgs({
		args,
		options: {
			provider: { type: 'string' },
			key: { type: 'string' },
			name: { type: 'string' },
			email: { type: 'string' },
			password: { type: 'string' },
			model: { type: 'string' },
			origin: { type: 'string' },
			port: { type: 'string' },
			url: { type: 'string' }
		}
	});

	const provider = values.provider ?? 'anthropic';
	if (!isProvider(provider)) {
		fail('--provider is anthropic, openai, openrouter, custom-openai, claude-plan or chatgpt-plan');
	}

	const { created } = initConfig();
	getDb();
	installCliShim();
	io.log(created ? `Created ${paths.home}` : `Using ${paths.home}`);

	let suggested = SETUP[provider].model;
	if (isPlan(provider)) {
		await setUpPlan(io, provider);
	} else if (provider === 'custom-openai') {
		if (!customOpenaiStatus().url || values.url) {
			const url =
				values.url ??
				(await ask(
					io,
					'Server address (Ollama: http://localhost:11434/v1, LM Studio: http://localhost:1234/v1)'
				));
			if (!url) fail('a server address is required (--url)');
			suggested = (await storeCustomOpenai(io, url, values.key ?? null))[0] ?? '';
		}
	} else {
		const { label, field } = API_KEYS[provider];
		if (!readConfig()[field]) {
			const key = values.key ?? (await askHidden(io, `${label} API key (${SETUP[provider].keys})`));
			if (!key) fail(`an ${label} API key is required`);
			await storeApiKey(io, provider, key);
		}
	}

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

	if (listPresets().length === 0) {
		const model = values.model ?? (await ask(io, 'Model', suggested));
		if (!model) fail('which model? Pass --model <id>');
		const preset = await addPreset({ provider, model });
		io.log(`Added model "${preset.name}".`);
	}

	const current = listenAddress();
	const port = values.port ? Number(values.port) : current.port;
	const origin =
		values.origin ??
		(await ask(
			io,
			'Public URL people will open (leave as is for this computer only)',
			readConfig().origin ?? `http://localhost:${port}`
		));
	updateConfig((c) => {
		c.port = port;
		c.origin = origin;
	});

	io.log(`
Done. Next:
  btw service install        run the gateway in the background (or \`btw start\` to try it)
  btw user create Anna anna@example.com    add family members
  open ${origin}

The gateway listens on http://${current.host}:${port}. To reach it from outside your home, point a
tunnel at that address (Tailscale Funnel, Cloudflare Tunnel, or your own VPS) and set its URL with
\`btw config set origin https://...\`.

${fullDiskAccessHint()}`);
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
	// adapter-node refuses bodies over 512 KB; attachments go up to MAX_MEDIA_BYTES. Every route
	// except the upload one keeps a 1 MB limit (src/hooks.server.ts).
	process.env.BODY_SIZE_LIMIT ??= String(MAX_MEDIA_BYTES + 1024 * 1024);
	io.log(
		`btw gateway: ${process.env.ORIGIN} (listening on ${process.env.HOST}:${process.env.PORT})`
	);
	// `pnpm dev` loads this file through Vite (src/hooks.server.ts), which can't follow a runtime
	// path: the built server is loaded by Node, as is.
	await import(/* @vite-ignore */ pathToFileURL(paths.server).href);
}

async function service(io: Io, action: string | undefined, args: string[]): Promise<void> {
	switch (action) {
		case 'install': {
			requireInit();
			if (!existsSync(paths.server))
				fail('no server build. In a source checkout, run `pnpm build` first.');
			if (args.includes('--dry-run')) {
				io.log(renderPlist());
				return;
			}
			const plist = await installService();
			const { origin } = listenAddress();
			io.log(`Installed ${plist}
The gateway starts now and at every login: ${origin}
Logs: ${logFile}
It runs with ${process.execPath}; run \`btw service install\` again after switching Node versions.

${fullDiskAccessHint()}`);
			return;
		}
		case 'uninstall':
			uninstallService();
			io.log('Removed the background service.');
			return;
		case 'restart':
			restartService();
			io.log('Restarted.');
			return;
		case 'status': {
			const status = serviceStatus();
			if (!status.installed) io.log('Not installed. Run `btw service install`.');
			else if (!status.loaded) io.log('Installed but not loaded. Run `btw service install`.');
			else
				io.log(
					status.pid
						? `Running (pid ${status.pid}).`
						: 'Loaded, not running. See `btw service logs`.'
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
			fail('usage: btw service install|uninstall|restart|status|logs [-f]');
	}
}

/**
 * Runs `btw` with these arguments and returns its exit code. Everything it reads and writes
 * besides its arguments and btw's own files goes through `io`. An error ends it with its message
 * and exit code 1.
 */
export async function runCli(argv: string[], io: Io): Promise<number> {
	try {
		return (await command(io, argv)) ?? 0;
	} catch (err) {
		io.error(`btw: ${err instanceof Error ? err.message : String(err)}`);
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

		case 'init': {
			// Non-interactive part of `btw setup`, kept for scripts.
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
				for (const key of apiKeyStatuses()) {
					const where =
						key.source === 'config' ? 'key set' : key.source === 'env' ? `key from ${key.env}` : '';
					const shown = where
						? `${where}${key.hint ? ` (…${key.hint})` : ''}`
						: `no key (btw key set ${key.provider})`;
					row(key.provider, shown);
				}
				const custom = customOpenaiStatus();
				row(
					'custom-openai',
					custom.url
						? `${custom.url}${custom.source === 'env' ? ` from ${CUSTOM_OPENAI_ENV.url}` : ''}, ${custom.hasKey ? `with a key${custom.hint ? ` (…${custom.hint})` : ''}` : 'no key'}`
						: 'no server (btw key set custom-openai <url>)'
				);
				const claude = claudeExecutable();
				row(
					'claude',
					claude
						? `Claude Code at ${claude} (btw claude-plan status checks its sign-in)`
						: 'no Claude Code found (btw claude-plan setup installs it)'
				);
				const codex = codexExecutable();
				row(
					'codex',
					codex
						? `Codex at ${codex} (btw chatgpt-plan status checks its sign-in)`
						: 'no Codex found (btw chatgpt-plan setup installs it)'
				);
				const images = imageGenerationStatus();
				row('images', `${images.model}${images.problem ? ` (${images.problem})` : ''}`);
				row('embeddings', embeddingStatus());
				row('env', Object.keys(config.commandEnv ?? {}).join(', ') || '-');
				return;
			}
			if (action !== 'set') {
				fail(
					'usage: btw config [set <host|port|origin|image-model|embeddings|claude-path|codex-path> <value>]'
				);
			}
			const key = positional(
				rest,
				0,
				'host|port|origin|image-model|embeddings|claude-path|codex-path'
			);
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
					const current = parseImageModel(c.imageModel || DEFAULT_IMAGE_MODEL);
					const { provider, model } = parseImageModel(value, current.provider);
					c.imageModel = `${provider}/${model}`;
				} else if (key === 'claude-path') c.claudePath = value;
				else if (key === 'codex-path') c.codexPath = value;
				else
					fail(
						'you can set host, port, origin, image-model, embeddings, claude-path or codex-path'
					);
			});
			if (key === 'claude-path') {
				await requireClaudePlan(io);
				return;
			}
			if (key === 'codex-path') {
				await requireChatGptPlan(io);
				return;
			}
			if (key === 'image-model') {
				const { model, problem } = imageGenerationStatus();
				io.log(`Pictures are now made with ${model}.${problem ? ` ${problem}` : ''}`);
				return;
			}
			io.log(`Set ${key}. Run \`btw service restart\` if the service is running.`);
			return;
		}

		case 'key': {
			requireInit();
			const provider = rest[0] ?? '';
			const names = [...Object.keys(API_KEYS), 'custom-openai'].join('|');
			if (provider === 'custom-openai' && action === 'rm') {
				removeCustomOpenai();
				const fallback = io.env[CUSTOM_OPENAI_ENV.url]
					? ` btw uses ${CUSTOM_OPENAI_ENV.url} from the environment now.`
					: '';
				io.log(`Removed the Custom OpenAI server.${fallback}`);
				return;
			}
			if (provider === 'custom-openai' && action === 'set') {
				const url = positional(rest, 1, 'url');
				await storeCustomOpenai(io, url, rest[2] ?? null);
				return;
			}
			if ((action !== 'set' && action !== 'rm') || !isApiKeyProvider(provider)) {
				fail(
					`usage: btw key set <${names}> [key] | btw key rm <${names}> (custom-openai takes its <url> before the key)`
				);
			}
			const { label, env } = API_KEYS[provider];
			if (action === 'rm') {
				removeApiKey(provider);
				const fallback = io.env[env] ? ` btw uses ${env} from the environment now.` : '';
				io.log(`Removed the ${label} API key.${fallback}`);
				return;
			}
			const key = rest[1] || (await askHidden(io, `${label} API key`));
			if (!key) fail('no key given');
			await storeApiKey(io, provider, key);
			return;
		}

		case 'claude-plan':
		case 'chatgpt-plan':
			return planCommand(io, group, action);

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
			} else fail('usage: btw env set|rm|list');
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
			} else if (action === 'list') {
				for (const u of listUsers()) {
					io.log(`${u.name}\t${u.email}${u.isAdmin ? '\tadmin' : ''}`);
				}
			} else fail('usage: btw user create|passwd|admin|rm|list');
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
				const model = positional(positionals, 0, 'model');
				const cw = values['context-window'];
				const preset = await addPreset({
					provider: values.provider,
					model,
					name: values.name,
					contextWindow: cw ? Number(cw) : null
				});
				io.log(`Added "${preset.name}" (context ${formatTokens(effectiveContextWindow(preset))}).`);
			} else if (action === 'edit') {
				const cw = values['context-window'];
				const preset = await editPreset(positional(positionals, 0, 'name|id'), {
					provider: values.provider,
					model: values.model,
					name: values.name,
					contextWindow: cw === undefined ? undefined : cw === 'auto' ? null : Number(cw)
				});
				io.log(
					`Saved "${preset.name}" (${preset.provider}/${preset.model}, context ${formatTokens(effectiveContextWindow(preset))}). Chats already on it keep what they had.`
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
						`${p.name}\t${p.provider}/${p.model}\tcontext ${formatTokens(effectiveContextWindow(p))}${override}\t${p.id}${isDefault}`
					);
				}
			} else fail('usage: btw preset add|edit|rm|default|list');
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
						`"${name}" is turned off in this profile, so new chats won't list it. Turn it on with \`btw skill enable ${name}\`.`
					);
				}
			} else if (action === 'list') {
				const slug = values.profile || io.env.BTW_PROFILE;
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
				if (!positionals.length) fail('missing <name>. See `btw help`.');
				const known = new Set(scanSkills(profileSkillsDir(profile.slug)).skills.map((s) => s.name));
				const unknown = positionals.filter((name) => !known.has(name));
				if (unknown.length) fail(`no skill named ${unknown.join(', ')}. See \`btw skill list\`.`);
				setSkillsEnabled(profile.id, positionals, action === 'enable');
				io.log(
					`${positionals.join(', ')}: ${action === 'enable' ? 'on' : 'off'} for new chats in ${profile.name}.`
				);
			} else fail('usage: btw skill new|list|enable|disable');
			return;
		}

		case 'trigger':
			requireInit();
			return triggerCommand(io, action, rest);

		case 'memory':
			requireInit();
			return memoryCommand(io, argv.slice(1));

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

		case 'view': {
			const dir = io.env.BTW_VIEW_DIR;
			if (!dir)
				fail("`btw view` only works in the agent's commands: it shows images to the agent.");
			const files = argv.slice(1);
			if (!files.length) fail('usage: btw view <image>...');
			let failed = false;
			for (const [i, file] of files.entries()) {
				try {
					io.log(await viewImage(file, dir, io.cwd));
				} catch (err) {
					failed = true;
					const message = err instanceof Error ? err.message : String(err);
					io.error(`btw: can't show ${file}: ${message.replace(/\.+$/, '')}.`);
					if (err instanceof ViewLimitError) {
						const rest = files.slice(i + 1);
						if (rest.length) io.error(`btw: not shown either: ${rest.join(' ')}`);
						break;
					}
				}
			}
			return failed ? 1 : 0;
		}

		default:
			fail(`unknown command "${group}". See \`btw help\`.`);
	}
}
