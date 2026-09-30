/*
 * Commands that only look: listing, reading and searching files, and nolune's own commands that
 * read. Auto mode (command-safety.ts) lets them run without asking its model, the way Claude
 * Code's auto mode lets its read-only tools through, so a turn that mostly looks around isn't
 * slowed down by a model call per command.
 *
 * Anything this can't read with certainty goes to the check instead: a wrong "no" costs one model
 * call, a wrong "yes" would skip the check. So the shell is read strictly: no substitutions, no
 * redirections but to /dev/null, no brace expansion or subshells, and only programs on the list,
 * without the flags that make them write or run something.
 */

/** A word of a command as the shell will see it once quotes are gone. */
interface Word {
	text: string;
	/** It has unquoted glob characters or expansions, which can turn into any words, flags too. */
	open: boolean;
}

type Token = { word: Word } | { op: 'pipe' | 'and' | 'or' | 'semi' };

interface Rule {
	/** Arguments that make the program write or run something. */
	forbid?: RegExp;
	/** Only these arguments; nothing else. */
	only?: RegExp;
	/** The program has forbidden flags, so a glob or expansion that might become one isn't safe. */
	closed?: boolean;
	maxArgs?: number;
}

const PROGRAMS: Record<string, Rule> = {
	ls: {},
	pwd: {},
	cat: {},
	head: {},
	tail: {},
	wc: {},
	stat: {},
	du: {},
	df: {},
	whoami: {},
	id: {},
	uname: {},
	uptime: {},
	sw_vers: {},
	which: {},
	realpath: {},
	readlink: {},
	basename: {},
	dirname: {},
	echo: {},
	printf: {},
	grep: {},
	egrep: {},
	fgrep: {},
	diff: {},
	cmp: {},
	cut: {},
	tr: {},
	nl: {},
	md5: {},
	md5sum: {},
	shasum: {},
	sha1sum: {},
	sha256sum: {},
	mdls: {},
	mdfind: {},
	// `--pre` runs a program on every file.
	rg: { forbid: /^--pre/, closed: true },
	// `-C` compiles a magic file, written to the working folder.
	file: { forbid: /^(-[a-zA-Z]*C|--compile)/, closed: true },
	find: {
		forbid: /^-(exec|execdir|ok|okdir|delete|fprint|fprint0|fprintf|fls)$/,
		closed: true
	},
	// `-o` writes the output to a file; `--compress-program` runs one.
	sort: { forbid: /^(-[a-zA-Z]*o|--output|--compress)/, closed: true },
	// A date as an argument sets the clock (with sudo); a format or UTC only reads it.
	date: { only: /^(\+.*|-u)$/, closed: true },
	cd: { maxArgs: 1 }
};

/** `nolune`'s commands that only read, by subcommand; `''` is the subcommand left out. */
const NOLUNE_READS: Record<string, readonly string[] | 'any'> = {
	view: 'any',
	memory: ['', 'list', 'search', 'show'],
	agent: ['list', 'watch'],
	trigger: ['list', 'show'],
	soul: ['', 'show'],
	skill: ['list'],
	profile: ['list'],
	help: 'any'
};

/**
 * Paths where secrets live. Reading them isn't read-only in the sense that matters: the agent
 * would have a key or password in hand. The check decides whether the request needs it.
 */
const SENSITIVE =
	/(^|[/~])\.(ssh|gnupg|aws|azure|kube|docker|netrc|npmrc|pypirc|pgpass|git-credentials)(\/|$)|(^|[/~])\.env\b|id_(rsa|dsa|ecdsa|ed25519)|keychain|config\.json|cookies|login data|credential|secret|passw|token|shadow|sudoers|environ|\.pem$|\.key$/i;

/** Longer than this, it's no quick look. */
const MAX_CHARS = 2000;

const NAME = /[A-Za-z_][A-Za-z0-9_]*/y;

/**
 * The command's words and operators, or null when it uses anything this doesn't read with
 * certainty: substitutions, subshells, brace expansion, here-documents, redirections other than to
 * /dev/null, background `&`, several lines.
 */
function lex(command: string): Token[] | null {
	const tokens: Token[] = [];
	let text = '';
	let open = false;
	let inWord = false;
	let i = 0;

	const end = () => {
		if (inWord) tokens.push({ word: { text, open } });
		text = '';
		open = false;
		inWord = false;
	};
	/** `$NAME` or `${NAME}` at `i`, the only expansions let through; its length, or 0. */
	const variable = (): number => {
		if (command[i + 1] === '{') {
			NAME.lastIndex = i + 2;
			const m = NAME.exec(command);
			return m && command[i + 2 + m[0].length] === '}' ? m[0].length + 3 : 0;
		}
		NAME.lastIndex = i + 1;
		const m = NAME.exec(command);
		return m ? m[0].length + 1 : 0;
	};
	/** A redirection at `i` (after its fd, if any): its length when it goes nowhere, else 0. */
	const redirection = (): number => {
		const rest = command.slice(i);
		const m = /^(&?>>?|>&)(\s*\/dev\/null(?=[\s;|&]|$)|[12](?=[\s;|&]|$))/.exec(rest);
		if (!m) return 0;
		// `>&1` and `>&2` point at another descriptor; `> 2` would be a file called 2.
		if (/^[12]$/.test(m[2]) && !m[1].endsWith('&')) return 0;
		return m[0].length;
	};

	while (i < command.length) {
		const c = command[i];
		if (c === ' ' || c === '\t') {
			end();
			i++;
		} else if (c === '\n' || c === '\r') {
			return null;
		} else if (c === "'") {
			const close = command.indexOf("'", i + 1);
			if (close < 0) return null;
			text += command.slice(i + 1, close);
			inWord = true;
			i = close + 1;
		} else if (c === '"') {
			i++;
			inWord = true;
			for (;;) {
				if (i >= command.length) return null;
				const d = command[i];
				if (d === '"') break;
				if (d === '`') return null;
				if (d === '\\') {
					const next = command[i + 1];
					if (next === undefined || next === '\n') return null;
					text += '$`"\\'.includes(next) ? next : `\\${next}`;
					i += 2;
				} else if (d === '$') {
					const length = variable();
					if (!length) return null;
					text += command.slice(i, i + length);
					i += length;
				} else {
					text += d;
					i++;
				}
			}
			i++;
		} else if (c === '\\') {
			const next = command[i + 1];
			if (next === undefined || next === '\n' || next === '\r') return null;
			text += next;
			inWord = true;
			i += 2;
		} else if (c === '$') {
			const length = variable();
			if (!length) return null;
			// Unquoted, it's split into words, which might start with a dash.
			text += command.slice(i, i + length);
			open = inWord = true;
			i += length;
		} else if (c === '*' || c === '?' || c === '[' || c === ']' || c === '^') {
			text += c;
			open = inWord = true;
			i++;
		} else if (c === '#' && !inWord) {
			// A comment: the rest of the line is ignored.
			break;
		} else if (c === '#') {
			// zsh's extended globs use it.
			text += c;
			open = true;
			i++;
		} else if (c === '|') {
			end();
			const two = command[i + 1];
			if (two === '|') {
				tokens.push({ op: 'or' });
				i += 2;
			} else {
				tokens.push({ op: 'pipe' });
				// `|&` pipes stderr too.
				i += two === '&' ? 2 : 1;
			}
		} else if (c === ';') {
			end();
			if (command[i + 1] === ';') return null;
			tokens.push({ op: 'semi' });
			i++;
		} else if (c === '&') {
			if (command[i + 1] === '&') {
				end();
				tokens.push({ op: 'and' });
				i += 2;
			} else {
				end();
				const length = redirection();
				if (!length) return null;
				i += length;
			}
		} else if (c === '>') {
			// A lone 1 or 2 right before it is the descriptor it redirects.
			if (inWord && !open && /^[12]$/.test(text) && /[12]/.test(command[i - 1])) {
				text = '';
				inWord = false;
			}
			end();
			const length = redirection();
			if (!length) return null;
			i += length;
		} else if ('<`(){}'.includes(c)) {
			return null;
		} else {
			text += c;
			inWord = true;
			i++;
		}
	}
	end();
	return tokens;
}

function isNoluneRead(args: Word[]): boolean {
	// Another profile's memory or soul isn't this chat's to look at without asking.
	if (args.some((a) => a.text === '--profile' || a.text.startsWith('--profile='))) return false;
	const [command, sub] = args.map((a) => a.text);
	if (command === undefined || !Object.hasOwn(NOLUNE_READS, command)) return false;
	const reads = NOLUNE_READS[command];
	if (reads === 'any') return true;
	// Left out (`nolune memory`, `nolune soul --json`), the subcommand is the one that reads.
	if (sub === undefined || sub.startsWith('-')) return reads.includes('');
	return sub !== '' && reads.includes(sub);
}

function isReadOnlySegment(words: Word[]): boolean {
	const [program, ...args] = words;
	if (!program || program.open) return false;
	if (program.text === 'nolune') return isNoluneRead(args);
	if (!Object.hasOwn(PROGRAMS, program.text)) return false;
	const rule = PROGRAMS[program.text];
	if (rule.maxArgs !== undefined && args.length > rule.maxArgs) return false;
	return args.every(
		(arg) =>
			!(rule.closed && arg.open) &&
			!rule.forbid?.test(arg.text) &&
			(!rule.only || rule.only.test(arg.text))
	);
}

/**
 * Whether the command only looks at things: every part of it is a program on the list, used
 * without flags that write or run something, joined by pipes, `&&`, `||` or `;`, and it touches no
 * place where secrets are kept.
 */
export function isReadOnlyCommand(command: string): boolean {
	if (command.length > MAX_CHARS) return false;
	const tokens = lex(command.trim());
	if (!tokens?.length) return false;
	const segments: Word[][] = [[]];
	for (const token of tokens) {
		if ('word' in token) segments.at(-1)!.push(token.word);
		else segments.push([]);
	}
	// A trailing `;` ends the last part; any other empty part isn't a command.
	const last = tokens.at(-1)!;
	if ('op' in last && last.op === 'semi') segments.pop();
	return segments.every(
		(words) => isReadOnlySegment(words) && !words.some((w) => SENSITIVE.test(w.text))
	);
}
