import { EventEmitter } from 'node:events';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { readConfig } from './config.ts';
import { appManaged, packageRoot, paths } from './paths.ts';

/*
 * Hearing about a new nolune. Each `v*` tag publishes the npm package and the macOS app together,
 * and announces them in a GitHub release (.github/workflows/publish.yml). The gateway asks GitHub
 * for the latest one about once a day and keeps what it hears in `latest-release.json`: admins see
 * it in the web interface, with how to update this install, and the macOS app's menu reads the
 * same file. `nolune config set update-check off` stops the asking.
 */

/** GitHub's latest release, which leaves out drafts and pre-releases. */
export const RELEASES_URL = 'https://api.github.com/repos/triangle-int/nolune/releases/latest';

/** Where the release's links have to point: they're opened, in the browser and from the app. */
const RELEASES_PAGE = 'https://github.com/triangle-int/nolune/releases/';

/** The disk images publish.yml attaches to each release, by Node's name for the architecture. */
const DISK_IMAGES = {
	arm64: 'nolune-macos-apple-silicon.dmg',
	x64: 'nolune-macos-intel.dmg'
} as const;

/** The nolune running, as its package.json says. */
export const NOLUNE_VERSION: string = JSON.parse(
	readFileSync(join(packageRoot, 'package.json'), 'utf8')
).version;

const CHECK_EVERY_MS = 24 * 60 * 60_000;
/** How often the gateway sees whether a check is due. One that failed is tried again then. */
const TICK_MS = 60 * 60_000;
/** The first look waits until the gateway has started. */
const FIRST_TICK_MS = 30_000;
const TIMEOUT_MS = 15_000;

export interface Release {
	/** Without the v, like `0.4.0`. */
	version: string;
	/** The release's page, with its notes. */
	url: string;
	publishedAt: string | null;
	/** The macOS app's disk images, by `process.arch`. */
	downloads: Partial<Record<keyof typeof DISK_IMAGES, string>>;
}

/** What `latest-release.json` keeps: when GitHub was last asked, and what it said (null: none yet). */
export interface SavedRelease {
	checkedAt: string;
	latest: Release | null;
}

/** How this nolune was installed, which decides how it's updated. */
export type InstallKind = 'app' | 'npm' | 'source';

/** What updates each install at a terminal. The app is downloaded instead. */
const UPDATE_COMMANDS: Record<InstallKind, string | null> = {
	npm: 'npm install -g nolune@latest && nolune service restart',
	source: 'git pull && pnpm install && pnpm build',
	app: null
};

/** A release newer than the one running, for admins. */
export interface Update {
	current: string;
	version: string;
	url: string;
	install: InstallKind;
	/** What updates it, at a terminal; null for the app, which is downloaded. */
	command: string | null;
	/** For the app: the disk image for this Mac, when the release has one. */
	download: string | null;
}

const holder = globalThis as unknown as {
	__noluneUpdateChecks?: boolean;
	__noluneUpdates?: EventEmitter;
};
const emitter = (holder.__noluneUpdates ??= new EventEmitter().setMaxListeners(0));

/** Called when GitHub names a release other than the one it named before. */
export function onReleaseFound(listener: () => void): () => void {
	emitter.on('found', listener);
	return () => emitter.off('found', listener);
}

/** Whether version `a` comes after `b`. Both are `major.minor.patch`, with or without a v. */
export function isNewer(a: string, b: string): boolean {
	const parts = (version: string) =>
		version
			.replace(/^v/, '')
			.split(/[.+-]/, 3)
			.map((n) => Number.parseInt(n, 10) || 0);
	const [x, y] = [parts(a), parts(b)];
	for (let i = 0; i < 3; i++) {
		if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) > (y[i] ?? 0);
	}
	return false;
}

export function installKind(): InstallKind {
	if (appManaged()) return 'app';
	// A checkout runs the CLI from source; the npm package and the app ship it built.
	if (existsSync(join(packageRoot, 'packages', 'cli', 'src', 'index.ts'))) return 'source';
	return 'npm';
}

/** Unset or broken config counts as on, as before setup. */
export function updateChecksOn(): boolean {
	try {
		return readConfig().updateCheck !== false;
	} catch {
		return true;
	}
}

export function savedRelease(): SavedRelease | null {
	try {
		const saved = JSON.parse(readFileSync(paths.latestRelease, 'utf8')) as SavedRelease;
		return typeof saved.checkedAt === 'string' ? saved : null;
	} catch {
		return null;
	}
}

function saveRelease(latest: Release | null): void {
	const before = savedRelease()?.latest?.version ?? null;
	mkdirSync(paths.home, { recursive: true });
	const temp = `${paths.latestRelease}.tmp`;
	const saved: SavedRelease = { checkedAt: new Date().toISOString(), latest };
	writeFileSync(temp, JSON.stringify(saved, null, '\t') + '\n');
	renameSync(temp, paths.latestRelease);
	if ((latest?.version ?? null) !== before) emitter.emit('found');
}

/** Forgets what GitHub said, when the checks are turned off. */
export function forgetRelease(): void {
	rmSync(paths.latestRelease, { force: true });
}

/** A link from GitHub's answer, if it's to one of nolune's releases. */
function releaseLink(value: unknown): string | undefined {
	return typeof value === 'string' && value.startsWith(RELEASES_PAGE) ? value : undefined;
}

function parseRelease(body: unknown): Release | null {
	if (typeof body !== 'object' || body === null) return null;
	const { tag_name, html_url, published_at, assets } = body as Record<string, unknown>;
	const version = typeof tag_name === 'string' ? tag_name.replace(/^v/, '') : '';
	const url = releaseLink(html_url);
	if (!/^\d+\.\d+\.\d+$/.test(version) || !url) return null;
	const downloads: Release['downloads'] = {};
	const files = Array.isArray(assets) ? (assets as Record<string, unknown>[]) : [];
	for (const [arch, name] of Object.entries(DISK_IMAGES)) {
		const link = releaseLink(files.find((file) => file?.name === name)?.browser_download_url);
		if (link) downloads[arch as keyof typeof DISK_IMAGES] = link;
	}
	return {
		version,
		url,
		publishedAt: typeof published_at === 'string' ? published_at : null,
		downloads
	};
}

/**
 * Asks GitHub for the latest release and saves what it says. Throws when GitHub can't be reached
 * or its answer makes no sense, and leaves what was saved before.
 */
export async function checkForUpdate(url = RELEASES_URL): Promise<Release | null> {
	const res = await fetch(url, {
		headers: { accept: 'application/vnd.github+json', 'user-agent': `nolune/${NOLUNE_VERSION}` },
		signal: AbortSignal.timeout(TIMEOUT_MS)
	});
	// No release yet.
	if (res.status === 404) {
		saveRelease(null);
		return null;
	}
	if (!res.ok) throw new Error(`GitHub answered ${res.status}`);
	const release = parseRelease(await res.json());
	if (!release) throw new Error('GitHub named no release nolune knows how to read');
	saveRelease(release);
	return release;
}

/** A newer release than this one, unless the checks are off. */
export function availableUpdate(): Update | null {
	if (!updateChecksOn()) return null;
	const latest = savedRelease()?.latest;
	if (!latest || !isNewer(latest.version, NOLUNE_VERSION)) return null;
	const install = installKind();
	const download =
		install === 'app' ? (latest.downloads[process.arch as keyof typeof DISK_IMAGES] ?? null) : null;
	return {
		current: NOLUNE_VERSION,
		version: latest.version,
		url: latest.url,
		install,
		command: UPDATE_COMMANDS[install],
		download
	};
}

/** For `nolune config`: the version, and whether there's a newer one. */
export function describeUpdates(): string {
	const current = NOLUNE_VERSION;
	if (!updateChecksOn()) return `${current} (not checking for new releases)`;
	const saved = savedRelease();
	if (!saved) return `${current} (not checked for new releases yet)`;
	const update = availableUpdate();
	if (!update) return `${current}, the newest (checked ${saved.checkedAt.slice(0, 10)})`;
	const how = update.command ?? "the menu bar's Download";
	return `${current}; ${update.version} is out (${update.url}). Update: ${how}`;
}

let warned = false;

async function checkWhenDue(): Promise<void> {
	if (!updateChecksOn()) return;
	const saved = savedRelease();
	if (saved && Date.now() - Date.parse(saved.checkedAt) < CHECK_EVERY_MS) return;
	try {
		await checkForUpdate();
		warned = false;
	} catch (err) {
		// Once until it works again: offline, it would say so every hour.
		if (!warned) console.error('[nolune] could not check for a new release:', err);
		warned = true;
	}
}

/**
 * Gateway only. Asks GitHub when the last answer is a day old, looking every hour, so a restart
 * doesn't ask again and a check that failed is tried again within the hour.
 */
export function startUpdateChecks(): void {
	if (holder.__noluneUpdateChecks) return;
	holder.__noluneUpdateChecks = true;
	setTimeout(checkWhenDue, FIRST_TICK_MS).unref();
	setInterval(checkWhenDue, TICK_MS).unref();
}
