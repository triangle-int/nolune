#!/usr/bin/env node
import { existsSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import {
	API_KEYS,
	ApiKeyError,
	DEFAULT_IMAGE_MODEL,
	DEFAULT_PORT,
	MAX_MEDIA_BYTES,
	addPreset,
	apiKeyStatuses,
	checkApiKey,
	configExists,
	createSkill,
	createUser,
	deleteUser,
	effectiveContextWindow,
	generatePassword,
	getDb,
	getDefaultPreset,
	getProfileBySlug,
	imageGenerationStatus,
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
	removePreset,
	saveApiKey,
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
import { GENERATE_HELP, generateCommand } from './generate.ts';
import { ask, askHidden } from './input.ts';
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

const HELP = `btw - a family agent that runs on this computer

Getting started
  btw setup [--provider anthropic|openai]    interactive first-time setup (key, your account, model);
                                             chats run on Claude unless you pick openai
  btw start                                  run the gateway in the foreground
  btw service install|uninstall|restart|status|logs [-f]
                                             run it in the background at login (macOS)

Settings (${paths.home})
  btw config                                 show address, port and what's configured
  btw config set <host|port|origin> <value>  origin = the public URL people open
  btw config set image-model <provider/model>  for pictures, e.g. openai/gpt-image-2.5-flare
  btw key set <anthropic|openai> [key]       store an API key (prompts if omitted) after checking
                                             it; OpenAI's runs GPT chats and makes pictures. Admins
                                             can also do this on the web, under Models & keys
  btw key rm <anthropic|openai>              remove a stored key (the environment's is used, if set)
  btw env set <NAME> <value>                 extra env var for agent commands (e.g. FIRECRAWL_API_KEY)
  btw env rm <NAME> | btw env list

Users (web sign-up is disabled; this is the only way to add people)
  btw user create <name> <email> [--password P] [--admin]
  btw user passwd <name|email> [--password P]
  btw user admin <name|email> [--off]
  btw user rm <name|email>
  btw user list

Model presets (shared by all profiles)
  btw preset add <model> [--provider anthropic|openai] [--name N] [--context-window TOKENS]
                                             the provider checks the model id first (anthropic
                                             unless given); OpenAI models other than the
                                             flagships need --context-window
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

${GENERATE_HELP}

Inside agent commands (BTW_PROFILE is set, so --profile can be left out)
  btw view <image>...                        show images to the agent: they're attached to the
                                             command's result (HEIC and big photos are converted)

${AGENT_HELP}`;

/** What `btw setup` suggests for each provider's first preset, and where its keys are made. */
const SETUP: Record<Provider, { model: string; keys: string }> = {
	anthropic: { model: 'claude-opus-5-5', keys: 'console.anthropic.com > API keys' },
	openai: { model: 'gpt-6-astra', keys: 'platform.openai.com > API keys' }
};

function fail(message: string): never {
	console.error(`btw: ${message}`);
	process.exit(1);
}

function positional(args: string[], index: number, name: string): string {
	const value = args[index];
	if (!value) fail(`missing <${name}>. See \`btw help\`.`);
	return value;
}

/**
 * Checks a pasted key with its provider and saves it. A key the provider rejects isn't saved; one
 * it couldn't be asked about is, with a warning, so setup works while the provider is unreachable.
 */
async function storeApiKey(provider: ApiKeyProvider, pasted: string): Promise<void> {
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
		console.log(`Saved the ${label} API key.${warning ? ` ${warning}` : ''}`);
	} catch (err) {
		if (!(err instanceof ApiKeyError) || err.reason !== 'unchecked') fail((err as Error).message);
		saveApiKey(provider, key);
		console.log(`Saved the ${label} API key without checking it. ${err.message}`);
	}
}

function requireInit(): void {
	if (!configExists()) fail('not set up yet. Run `btw setup` first.');
}

function resolveProfileSlug(flag: string | undefined): string {
	const slug = flag || process.env.BTW_PROFILE;
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

async function setup(args: string[]): Promise<void> {
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
			port: { type: 'string' }
		}
	});

	const provider = values.provider ?? 'anthropic';
	if (!isProvider(provider)) fail('--provider is anthropic or openai');
	const { label, field } = API_KEYS[provider];

	const { created } = initConfig();
	getDb();
	installCliShim();
	console.log(created ? `Created ${paths.home}` : `Using ${paths.home}`);

	if (!readConfig()[field]) {
		const key = values.key ?? (await askHidden(`${label} API key (${SETUP[provider].keys})`));
		if (!key) fail(`an ${label} API key is required`);
		await storeApiKey(provider, key);
	}

	const admin = listUsers().find((u) => u.isAdmin);
	if (admin) {
		console.log(`Admin account: ${admin.name} <${admin.email}>`);
	} else {
		const name = values.name ?? (await ask('Your name (the agent sees it on your messages)'));
		const email = values.email ?? (await ask('Your email (to sign in)'));
		const password = values.password ?? generatePassword();
		await createUser({ name, email, password, isAdmin: true });
		console.log(`Created your account. Password: ${values.password ? '(as given)' : password}`);
	}

	if (listPresets().length === 0) {
		const model = values.model ?? (await ask('Model', SETUP[provider].model));
		const preset = await addPreset({ provider, model });
		console.log(`Added model "${preset.name}".`);
	}

	const current = listenAddress();
	const port = values.port ? Number(values.port) : current.port;
	const origin =
		values.origin ??
		(await ask(
			'Public URL people will open (leave as is for this computer only)',
			readConfig().origin ?? `http://localhost:${port}`
		));
	updateConfig((c) => {
		c.port = port;
		c.origin = origin;
	});

	console.log(`
Done. Next:
  btw service install        run the gateway in the background (or \`btw start\` to try it)
  btw user create Anna anna@example.com    add family members
  open ${origin}

The gateway listens on http://${current.host}:${port}. To reach it from outside your home, point a
tunnel at that address (Tailscale Funnel, Cloudflare Tunnel, or your own VPS) and set its URL with
\`btw config set origin https://...\`.

${fullDiskAccessHint()}`);
}

async function start(): Promise<void> {
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
	console.log(
		`btw gateway: ${process.env.ORIGIN} (listening on ${process.env.HOST}:${process.env.PORT})`
	);
	await import(pathToFileURL(paths.server).href);
}

async function service(action: string | undefined, args: string[]): Promise<void> {
	switch (action) {
		case 'install': {
			requireInit();
			if (!existsSync(paths.server))
				fail('no server build. In a source checkout, run `pnpm build` first.');
			if (args.includes('--dry-run')) {
				console.log(renderPlist());
				return;
			}
			const plist = await installService();
			const { origin } = listenAddress();
			console.log(`Installed ${plist}
The gateway starts now and at every login: ${origin}
Logs: ${logFile}
It runs with ${process.execPath}; run \`btw service install\` again after switching Node versions.

${fullDiskAccessHint()}`);
			return;
		}
		case 'uninstall':
			uninstallService();
			console.log('Removed the background service.');
			return;
		case 'restart':
			restartService();
			console.log('Restarted.');
			return;
		case 'status': {
			const status = serviceStatus();
			if (!status.installed) console.log('Not installed. Run `btw service install`.');
			else if (!status.loaded) console.log('Installed but not loaded. Run `btw service install`.');
			else
				console.log(
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

async function main(argv: string[]): Promise<void> {
	const [group, action, ...rest] = argv;

	switch (group) {
		case undefined:
		case 'help':
		case '--help':
		case '-h':
			console.log(HELP);
			return;

		case 'setup':
			return setup(argv.slice(1));

		case 'start':
			return start();

		case 'service':
			return service(action, rest);

		case 'init': {
			// Non-interactive part of `btw setup`, kept for scripts.
			const { created } = initConfig();
			getDb();
			const shim = installCliShim();
			console.log(created ? `Created ${paths.config}` : `Config already exists: ${paths.config}`);
			console.log(`Database ready: ${paths.db}`);
			console.log(`Agent shim: ${shim}`);
			return;
		}

		case 'config': {
			requireInit();
			if (action === undefined) {
				const config = readConfig();
				const { host, port, origin } = listenAddress();
				console.log(`home       ${paths.home}`);
				console.log(`listen     http://${host}:${port}`);
				console.log(`origin     ${origin}`);
				for (const key of apiKeyStatuses()) {
					const where =
						key.source === 'config' ? 'key set' : key.source === 'env' ? `key from ${key.env}` : '';
					const shown = where
						? `${where}${key.hint ? ` (…${key.hint})` : ''}`
						: `no key (btw key set ${key.provider})`;
					console.log(`${key.provider.padEnd(10)} ${shown}`);
				}
				const images = imageGenerationStatus();
				console.log(`images     ${images.model}${images.problem ? ` (${images.problem})` : ''}`);
				console.log(`env        ${Object.keys(config.commandEnv ?? {}).join(', ') || '-'}`);
				return;
			}
			if (action !== 'set') fail('usage: btw config [set <host|port|origin|image-model> <value>]');
			const key = positional(rest, 0, 'host|port|origin|image-model');
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
				} else fail('you can set host, port, origin or image-model');
			});
			if (key === 'image-model') {
				const { model, problem } = imageGenerationStatus();
				console.log(`Pictures are now made with ${model}.${problem ? ` ${problem}` : ''}`);
				return;
			}
			console.log(`Set ${key}. Run \`btw service restart\` if the service is running.`);
			return;
		}

		case 'key': {
			requireInit();
			const provider = rest[0] ?? '';
			const names = Object.keys(API_KEYS).join('|');
			if ((action !== 'set' && action !== 'rm') || !isApiKeyProvider(provider)) {
				fail(`usage: btw key set <${names}> [key] | btw key rm <${names}>`);
			}
			const { label, env } = API_KEYS[provider];
			if (action === 'rm') {
				removeApiKey(provider);
				const fallback = process.env[env] ? ` btw uses ${env} from the environment now.` : '';
				console.log(`Removed the ${label} API key.${fallback}`);
				return;
			}
			const key = rest[1] || (await askHidden(`${label} API key`));
			if (!key) fail('no key given');
			await storeApiKey(provider, key);
			return;
		}

		case 'env': {
			requireInit();
			if (action === 'list') {
				for (const name of Object.keys(readConfig().commandEnv ?? {})) console.log(name);
			} else if (action === 'set') {
				const name = positional(rest, 0, 'NAME');
				const value = rest[1] ?? (await askHidden(name));
				if (!value) fail('no value given');
				updateConfig((c) => {
					c.commandEnv = { ...c.commandEnv, [name]: value };
				});
				console.log(`Set ${name} for agent commands.`);
			} else if (action === 'rm') {
				const name = positional(rest, 0, 'NAME');
				updateConfig((c) => {
					if (c.commandEnv) delete c.commandEnv[name];
				});
				console.log(`Removed ${name}.`);
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
				console.log(`Created ${name} <${email}>${values.admin ? ' (admin)' : ''}.`);
				if (!values.password) console.log(`Password: ${password}`);
			} else if (action === 'passwd') {
				const who = positional(positionals, 0, 'name|email');
				const password = values.password ?? generatePassword();
				await setPassword(who, password);
				console.log(`Password changed for ${who}.`);
				if (!values.password) console.log(`Password: ${password}`);
			} else if (action === 'admin') {
				const who = positional(positionals, 0, 'name|email');
				setAdmin(who, !values.off);
				console.log(`${who} is ${values.off ? 'no longer' : 'now'} an admin.`);
			} else if (action === 'rm') {
				const who = positional(positionals, 0, 'name|email');
				deleteUser(who);
				console.log(`Deleted ${who}.`);
			} else if (action === 'list') {
				for (const u of listUsers()) {
					console.log(`${u.name}\t${u.email}${u.isAdmin ? '\tadmin' : ''}`);
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
				console.log(
					`Added "${preset.name}" (context ${formatTokens(effectiveContextWindow(preset))}).`
				);
			} else if (action === 'rm') {
				removePreset(positional(positionals, 0, 'name|id'));
				console.log('Removed. Existing conversations keep working.');
			} else if (action === 'default') {
				const preset = setDefaultPreset(positional(positionals, 0, 'name|id'));
				console.log(`"${preset.name}" is now the default. New chats start with it.`);
			} else if (action === 'list') {
				const defaultId = getDefaultPreset()?.id;
				for (const p of listPresets()) {
					const override = p.contextWindow ? ' (override)' : '';
					const isDefault = p.id === defaultId ? '\tdefault' : '';
					console.log(
						`${p.name}\t${p.provider}/${p.model}\tcontext ${formatTokens(effectiveContextWindow(p))}${override}\t${p.id}${isDefault}`
					);
				}
			} else fail('usage: btw preset add|rm|default|list');
			return;
		}

		case 'profile':
			requireInit();
			return profileCommand(action, rest);

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
				const slug = values.global ? null : resolveProfileSlug(values.profile);
				const dir = slug ? profileSkillsDir(slug) : paths.globalSkills;
				const location = createSkill(dir, name, values.description ?? '');
				console.log(`Created ${location}`);
				if (slug && getProfileBySlug(slug)?.disabledSkills.includes(name)) {
					console.log(
						`"${name}" is turned off in this profile, so new chats won't list it. Turn it on with \`btw skill enable ${name}\`.`
					);
				}
			} else if (action === 'list') {
				const slug = values.profile || process.env.BTW_PROFILE;
				const profile = slug ? getProfileBySlug(slug) : undefined;
				const { skills, warnings } = listProfileSkills(
					slug ? profileSkillsDir(slug) : '/nonexistent',
					profile?.disabledSkills ?? []
				);
				for (const s of skills) {
					const state = profile ? `${s.enabled ? 'on' : 'off'}\t` : '';
					console.log(
						`${s.name}\t${state}${s.scope}\t~${s.tokens} tokens\t${s.location}\n  ${s.description}`
					);
				}
				for (const w of warnings) console.error(`warning: ${w}`);
				if (!skills.length) console.log('No skills.');
				else if (profile) {
					const on = skills.filter((s) => s.enabled);
					const tokens = on.reduce((n, s) => n + s.tokens, 0);
					console.log(
						`\n${on.length} of ${skills.length} on in ${profile.name}, about ${formatTokens(tokens)} tokens in every new chat.`
					);
				}
			} else if (action === 'enable' || action === 'disable') {
				if (values.global) fail('skills are turned on and off per profile. Pass --profile <slug>.');
				const profile = getProfileBySlug(resolveProfileSlug(values.profile))!;
				if (!positionals.length) fail('missing <name>. See `btw help`.');
				const known = new Set(scanSkills(profileSkillsDir(profile.slug)).skills.map((s) => s.name));
				const unknown = positionals.filter((name) => !known.has(name));
				if (unknown.length) fail(`no skill named ${unknown.join(', ')}. See \`btw skill list\`.`);
				setSkillsEnabled(profile.id, positionals, action === 'enable');
				console.log(
					`${positionals.join(', ')}: ${action === 'enable' ? 'on' : 'off'} for new chats in ${profile.name}.`
				);
			} else fail('usage: btw skill new|list|enable|disable');
			return;
		}

		case 'trigger':
			requireInit();
			return triggerCommand(action, rest);

		case 'memory':
			requireInit();
			return memoryCommand(argv.slice(1));

		case 'soul':
			requireInit();
			return soulCommand(argv.slice(1));

		case 'wake':
			requireInit();
			return wakeCommand(argv.slice(1));

		case 'agent':
			requireInit();
			return agentCommand(action, rest);

		case 'generate':
			return generateCommand(action, rest);

		case 'view': {
			const dir = process.env.BTW_VIEW_DIR;
			if (!dir)
				fail("`btw view` only works in the agent's commands: it shows images to the agent.");
			const files = argv.slice(1);
			if (!files.length) fail('usage: btw view <image>...');
			let failed = false;
			for (const [i, file] of files.entries()) {
				try {
					console.log(await viewImage(file, dir));
				} catch (err) {
					failed = true;
					const message = err instanceof Error ? err.message : String(err);
					console.error(`btw: can't show ${file}: ${message.replace(/\.+$/, '')}.`);
					if (err instanceof ViewLimitError) {
						const rest = files.slice(i + 1);
						if (rest.length) console.error(`btw: not shown either: ${rest.join(' ')}`);
						break;
					}
				}
			}
			if (failed) process.exit(1);
			return;
		}

		default:
			fail(`unknown command "${group}". See \`btw help\`.`);
	}
}

main(process.argv.slice(2)).catch((err: unknown) =>
	fail(err instanceof Error ? err.message : String(err))
);
