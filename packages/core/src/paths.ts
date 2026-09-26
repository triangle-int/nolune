import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const home = process.env.BTW_HOME || join(homedir(), '.btw-agent');

/**
 * Root of the btw-agent package: the repo checkout, or the installed npm package. Found by walking
 * up from this file, which works from source, from the SvelteKit build and from the bundled CLI.
 */
function findPackageRoot(): string {
	let dir = dirname(fileURLToPath(import.meta.url));
	for (;;) {
		const manifest = join(dir, 'package.json');
		if (existsSync(manifest)) {
			try {
				if (JSON.parse(readFileSync(manifest, 'utf8')).name === 'btw-agent') return dir;
			} catch {
				// not ours; keep walking
			}
		}
		const parent = dirname(dir);
		if (parent === dir) throw new Error('Could not find the btw-agent package root');
		dir = parent;
	}
}

export const packageRoot = findPackageRoot();

export const paths = {
	home,
	config: join(home, 'config.json'),
	db: join(home, 'btw.db'),
	bin: join(home, 'bin'),
	logs: join(home, 'logs'),
	profiles: join(home, 'profiles'),
	trash: join(home, 'trash'),
	/** Copies of the pictures and files shown in chats, named by their SHA-256. */
	media: join(home, 'media'),
	globalSkills: process.env.BTW_GLOBAL_SKILLS || join(homedir(), '.agents', 'skills'),
	/** Skills that ship with btw, such as `automations`. */
	builtinSkills: join(packageRoot, 'packages', 'core', 'skills'),
	/** Image templates for every profile; each profile can add its own (profileImageTemplatesDir). */
	globalImageTemplates: join(home, 'image-templates'),
	builtinImageTemplates: join(packageRoot, 'packages', 'core', 'image-templates'),
	migrations: join(packageRoot, 'packages', 'core', 'drizzle'),
	/** adapter-node output; `btw start` runs it. */
	server: join(packageRoot, 'build', 'index.js')
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
