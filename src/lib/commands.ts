/**
 * Plain-language labels for the shell commands the agent runs, so people who don't read shell
 * see "Reading notes.txt" instead of `cat ~/notes.txt`. A best guess from the first real
 * command; anything unknown is just "Running a command".
 */

export type StepIcon =
	| 'terminal'
	| 'file'
	| 'folder'
	| 'search'
	| 'globe'
	| 'clock'
	| 'book'
	| 'code'
	| 'pencil'
	| 'trash'
	| 'app'
	| 'package'
	| 'image'
	| 'laptop'
	| 'database';

export interface CommandDescription {
	label: string;
	icon: StepIcon;
}

const SKIP = new Set(['cd', 'export', 'set', 'source', '.', 'true', 'unset', 'local', 'pushd']);
const PREFIXES = new Set(['sudo', 'command', 'time', 'nohup', 'env', 'exec', 'caffeinate']);
/** Commands that say little on their own; a later command describes the work better. */
const FILLER = new Set(['sleep', 'echo', 'printf', 'wait', 'clear', 'test', '[']);

/** Splits a shell line into words, keeping quoted strings together (good enough for labels). */
function words(line: string): string[] {
	return [...line.matchAll(/"((?:[^"\\]|\\.)*)"|'([^']*)'|(\S+)/g)].map(
		(m) => m[1] ?? m[2] ?? m[3]
	);
}

/**
 * The command that best says what's going on, as words: the first one that isn't `cd`, an env
 * assignment or filler like `sleep`, without prefixes like `sudo`.
 */
function mainCommand(command: string): string[] {
	const candidates = command
		.split('\n')
		.map((line) => line.trim())
		.filter((line) => line && !line.startsWith('#'))
		.flatMap((line) => line.split(/\s*(?:&&|\|\||;|\|)\s*/))
		.map((segment) => {
			// Redirections like `2>/dev/null` or `> out.txt` aren't arguments.
			let parts = words(segment.replace(/\d*[<>]{1,2}&?\s*\S+/g, ''));
			while (parts.length && (/^[A-Za-z_][A-Za-z0-9_]*=/.test(parts[0]) || PREFIXES.has(parts[0])))
				parts = parts.slice(1);
			return parts;
		})
		.filter((parts) => parts.length && !SKIP.has(parts[0]));
	return candidates.find((parts) => !FILLER.has(parts[0])) ?? candidates[0] ?? [];
}

function basename(path: string): string {
	// `~/Downloads/*` names Downloads.
	const clean = path.replace(/(\/[^/]*[*?][^/]*)+$/, '').replace(/\/+$/, '');
	const name = clean.slice(clean.lastIndexOf('/') + 1) || clean;
	return name.length > 40 ? name.slice(0, 39) + '…' : name;
}

/** The last argument that isn't a flag, e.g. the file in `head -n 20 notes.txt`. */
function target(args: string[]): string | null {
	const found = args.filter((a) => !a.startsWith('-') && !/^\d+$/.test(a)).at(-1);
	return found ? basename(found) : null;
}

function host(url: string | undefined): string | null {
	if (!url) return null;
	try {
		return new URL(url).hostname.replace(/^www\./, '');
	} catch {
		return null;
	}
}

export function describeCommand(command: string | null): CommandDescription {
	if (!command) return { label: 'Getting ready', icon: 'terminal' };

	const skill = command.match(/([^/\s'"]+)\/SKILL\.md/);
	if (skill) return { label: `Reading the ${skill[1]} skill`, icon: 'book' };

	const [name = '', ...args] = mainCommand(command);
	const tool = basename(name);
	const redirect = command.match(/(?<![0-9&])>>?\s*([^\s|&;<>]+)/)?.[1];

	switch (tool) {
		case 'btw': {
			if (args[0] === 'trigger') {
				if (args[1] === 'list') return { label: 'Checking automations', icon: 'clock' };
				if (args[1] === 'add' || args[1] === 'create')
					return { label: 'Setting up an automation', icon: 'clock' };
				return { label: 'Updating an automation', icon: 'clock' };
			}
			if (args[0] === 'skill') return { label: 'Working on a skill', icon: 'book' };
			return { label: 'Using btw', icon: 'terminal' };
		}
		case 'cat':
		case 'echo':
		case 'printf':
		case 'tee':
			if (redirect && redirect !== '/dev/null')
				return { label: `Writing ${basename(redirect)}`, icon: 'pencil' };
			if (tool === 'tee' && target(args))
				return { label: `Writing ${target(args)}`, icon: 'pencil' };
			if (tool === 'cat') return { label: `Reading ${target(args) ?? 'a file'}`, icon: 'file' };
			return { label: 'Running a command', icon: 'terminal' };
		case 'head':
		case 'tail':
		case 'less':
		case 'more':
		case 'bat':
		case 'sed':
		case 'awk':
		case 'wc':
		case 'file':
		case 'stat':
		case 'jq':
		case 'xxd':
		case 'strings':
		case 'pdftotext':
		case 'textutil':
		case 'mdls':
		case 'plutil':
			return { label: `Reading ${target(args) ?? 'a file'}`, icon: 'file' };
		case 'ls':
		case 'tree':
		case 'find':
		case 'fd':
		case 'du':
		case 'mdfind':
		case 'locate': {
			const where = tool === 'find' ? args.find((a) => !a.startsWith('-')) : target(args);
			return where && where !== '.' && tool !== 'mdfind'
				? { label: `Looking in ${basename(where)}`, icon: 'folder' }
				: { label: 'Looking through files', icon: 'folder' };
		}
		case 'grep':
		case 'rg':
		case 'ag':
		case 'ack':
			return { label: 'Searching files', icon: 'search' };
		case 'curl':
		case 'wget':
		case 'http':
		case 'xh': {
			const site = host(args.find((a) => /^https?:\/\//.test(a)));
			return { label: site ? `Visiting ${site}` : 'Visiting a website', icon: 'globe' };
		}
		case 'firecrawl':
			return args.includes('search')
				? { label: 'Searching the web', icon: 'globe' }
				: { label: 'Reading a web page', icon: 'globe' };
		case 'open': {
			const url = args.find((a) => /^https?:\/\//.test(a));
			if (url) return { label: `Opening ${host(url) ?? 'a web page'}`, icon: 'globe' };
			const app = args[args.indexOf('-a') + 1];
			if (args.includes('-a') && app) return { label: `Opening ${app}`, icon: 'app' };
			return { label: `Opening ${target(args) ?? 'a file'}`, icon: 'app' };
		}
		case 'osascript': {
			const app = command.match(/application "([^"]+)"/)?.[1];
			return { label: app ? `Using ${app}` : 'Controlling an app', icon: 'app' };
		}
		case 'shortcuts':
			return { label: 'Running a Shortcut', icon: 'app' };
		case 'mkdir':
			return { label: 'Creating a folder', icon: 'folder' };
		case 'touch':
			return { label: `Creating ${target(args) ?? 'a file'}`, icon: 'pencil' };
		case 'cp':
		case 'rsync':
		case 'ditto':
			return { label: 'Copying files', icon: 'folder' };
		case 'mv':
			return { label: 'Moving files', icon: 'folder' };
		case 'rm':
		case 'rmdir':
		case 'trash':
			return { label: 'Deleting files', icon: 'trash' };
		case 'zip':
		case 'tar':
		case 'gzip':
			return { label: 'Packing files', icon: 'package' };
		case 'unzip':
		case 'gunzip':
			return { label: 'Unpacking files', icon: 'package' };
		case 'python':
		case 'python3':
		case 'node':
		case 'ruby':
		case 'perl':
		case 'bash':
		case 'sh':
		case 'zsh':
		case 'deno':
		case 'bun':
			return { label: 'Running a script', icon: 'code' };
		case 'brew':
		case 'npm':
		case 'pnpm':
		case 'pip':
		case 'pip3':
		case 'uv':
		case 'cargo':
		case 'gem':
			return args.includes('install') || args.includes('add')
				? { label: 'Installing software', icon: 'package' }
				: { label: `Using ${tool}`, icon: 'package' };
		case 'git':
			return { label: 'Using git', icon: 'code' };
		case 'sqlite3':
			return { label: 'Looking in a database', icon: 'database' };
		case 'ffmpeg':
		case 'sips':
		case 'magick':
		case 'convert':
		case 'exiftool':
			return { label: 'Working on media', icon: 'image' };
		case 'date':
		case 'cal':
			return { label: 'Checking the date', icon: 'clock' };
		case 'sleep':
			return { label: 'Waiting', icon: 'clock' };
		case 'sw_vers':
		case 'uname':
		case 'system_profiler':
		case 'hostname':
		case 'whoami':
		case 'uptime':
		case 'df':
		case 'top':
		case 'ps':
		case 'pmset':
		case 'networksetup':
		case 'ifconfig':
		case 'ping':
		case 'defaults':
			return { label: 'Checking the computer', icon: 'laptop' };
		case 'pbcopy':
		case 'pbpaste':
			return { label: 'Using the clipboard', icon: 'app' };
		case 'say':
			return { label: 'Speaking out loud', icon: 'app' };
		default:
			return { label: 'Running a command', icon: 'terminal' };
	}
}

/** The command's first line, for the compact technical label. */
export function firstLine(command: string, max = 120): string {
	const line = command.trim().split('\n')[0];
	return line.length > max ? line.slice(0, max) + '…' : line;
}

/** Reads `command` out of the tool input JSON while it is still streaming in. */
export function partialCommand(json: string): string | null {
	try {
		const input = JSON.parse(json) as { command?: unknown };
		return typeof input.command === 'string' ? input.command : null;
	} catch {
		const match = json.match(/"command"\s*:\s*"((?:[^"\\]|\\.)*)/);
		if (!match) return null;
		// The text may stop halfway through an escape like \u00e9; drop the unfinished tail.
		for (let cut = 0; cut <= 5; cut++) {
			try {
				return JSON.parse(`"${match[1].slice(0, match[1].length - cut)}"`) as string;
			} catch {
				// Try a shorter prefix.
			}
		}
		return match[1];
	}
}
