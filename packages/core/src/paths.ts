import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const home = process.env.NOLUNE_HOME || join(homedir(), '.nolune');

/**
 * Root of the nolune package: the repo checkout, or the installed npm package. Found by walking
 * up from this file, which works from source, from the SvelteKit build and from the bundled CLI.
 */
function findPackageRoot(): string {
	let dir = dirname(fileURLToPath(import.meta.url));
	for (;;) {
		const manifest = join(dir, 'package.json');
		if (existsSync(manifest)) {
			try {
				if (JSON.parse(readFileSync(manifest, 'utf8')).name === 'nolune') return dir;
			} catch {
				// not ours; keep walking
			}
		}
		const parent = dirname(dir);
		if (parent === dir) throw new Error('Could not find the nolune package root');
		dir = parent;
	}
}

export const packageRoot = findPackageRoot();

export const paths = {
	home,
	config: join(home, 'config.json'),
	/**
	 * Codex's home for chats on the ChatGPT plan (chatgpt-plan.ts), apart from ~/.codex: its
	 * ChatGPT sign-in, which only Codex reads, and the chats' threads.
	 */
	codexHome: join(home, 'codex'),
	db: join(home, 'nolune.db'),
	bin: join(home, 'bin'),
	logs: join(home, 'logs'),
	profiles: join(home, 'profiles'),
	trash: join(home, 'trash'),
	/** Copies of the pictures and files shown in chats, named by their SHA-256. */
	media: join(home, 'media'),
	globalSkills: process.env.NOLUNE_GLOBAL_SKILLS || join(homedir(), '.agents', 'skills'),
	/** Skills that ship with nolune, such as `automations`. */
	builtinSkills: join(packageRoot, 'packages', 'core', 'skills'),
	/** Image templates for every profile; each profile can add its own (profileImageTemplatesDir). */
	globalImageTemplates: join(home, 'image-templates'),
	builtinImageTemplates: join(packageRoot, 'packages', 'core', 'image-templates'),
	migrations: join(packageRoot, 'packages', 'core', 'drizzle'),
	/** adapter-node output; `nolune start` runs it. */
	server: join(packageRoot, 'build', 'index.js'),
	/**
	 * Where the gateway runs `nolune` commands for the CLI, so they don't load all of nolune each time.
	 * Its folder is private: only this user can connect.
	 */
	cliSocket: join(home, 'run', 'cli.sock')
};

/**
 * How to run the CLI: from source in a checkout (always current), otherwise the bundled
 * dist/cli.js that ships in the npm package.
 */
export function cliCommand(): string[] {
	const source = join(packageRoot, 'packages', 'cli', 'src', 'index.ts');
	if (existsSync(source)) return [process.execPath, '--no-warnings', source];
	return [process.execPath, join(packageRoot, 'dist', 'cli.js')];
}

export function profileDir(slug: string): string {
	return join(paths.profiles, slug);
}

export function profileSkillsDir(slug: string): string {
	return join(paths.profiles, slug, 'skills');
}

export function profileImageTemplatesDir(slug: string): string {
	return join(paths.profiles, slug, 'image-templates');
}

/** Who nolune is for this profile's family; it opens every chat's system prompt. */
export function profileSoulFile(slug: string): string {
	return join(paths.profiles, slug, 'soul.md');
}

/** Long-term memory: one Markdown note per topic. */
export function profileMemoryDir(slug: string): string {
	return join(paths.profiles, slug, 'memories');
}

/** Where the files of each chat folder are saved, one folder per chat folder. */
export function profileFoldersDir(slug: string): string {
	return join(paths.profiles, slug, 'folders');
}
