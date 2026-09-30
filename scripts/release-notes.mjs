// Writes a release's notes with Claude, from the commits since the tag before it: each is a
// squash-merged pull request, whose message says what changed and why. The notes follow
// release-notes/example.md (0.2.0's) and are for the families who use nolune, not for developers.
//
//   ANTHROPIC_API_KEY=... node scripts/release-notes.mjs [ref] [previous tag]
//
// The release is `ref` (HEAD by default: the next release, as a preview), named after the version
// in its package.json; the commits are those since `previous tag` (the last tag before `ref` by
// default). Prints Markdown: an opening paragraph, a {{downloads}} line where the Publish workflow
// puts the download links, then what's new and how to upgrade. Exits non-zero with the reason on
// stderr when it can't; the workflow then falls back to GitHub's list of pull requests.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import Anthropic from '@anthropic-ai/sdk';

const MODEL = 'claude-opus-5-5';

const example = readFileSync(new URL('release-notes/example.md', import.meta.url), 'utf8');

const SYSTEM = `You write the release notes for nolune, shown on its GitHub release page.

nolune is a small agent that lives on a family's computer and does things for them, from a shared web chat. The person reading is usually whoever set it up for the family, or a family member deciding whether to update. Most are not developers.

You'll get the release's version, the one before it, and the commits between them. Each commit is a squash-merged pull request: its title says what changed, its body how and why. Write from those only: never invent a feature, a number, a command or a setting, and use commands, settings and menu names exactly as the commits spell them.

How the notes read:
- An opening paragraph of one or two sentences: what this release means for a family, in plain words.
- Then a line with exactly {{downloads}} and nothing else. The download links go there.
- "## What's new": a "### " section with one fitting emoji for each change people will notice, the biggest first. Say what they can do now and where to find it, in the second person, in short sentences or a few bullets. Group related commits into one section.
- "## Upgrading from <previous version>", only with steps someone must take: always how to update from npm (\`npm install -g nolune@latest\`, then \`nolune service restart\`), plus anything a change asks of them, like signing in again or a setting that moved.
- "## Also in this release", for smaller changes worth a line: fixes people would have hit, the site, the docs. Leave it out when there are none.
- Leave out what only developers see (CI, refactors, tests, moving code around, dependency bumps) and the release's own version bump.
- No title (GitHub shows the release's name), no download links, no pull request numbers, commit hashes or file paths unless someone types them, no closing line.

Answer with the notes in Markdown and nothing else.

This is 0.2.0's, as an example of the tone and the shape:

<example>
${example}</example>`;

function git(...args) {
	return execFileSync('git', args, { encoding: 'utf8' }).trim();
}

/** The commits in a range, without the trailers that only say who wrote them. */
function commits(from, to) {
	return git('log', '--reverse', '--format=%s%n%n%b%x1e', `${from}..${to}`)
		.split('\x1e')
		.map((commit) =>
			commit
				.split('\n')
				.filter((line) => !/^(Co-authored-by|Claude-Session|Signed-off-by):/i.test(line))
				.join('\n')
				.trim()
		)
		.filter(Boolean);
}

async function main() {
	const [ref = 'HEAD', given] = process.argv.slice(2);
	const previous = given ?? git('describe', '--tags', '--abbrev=0', `${ref}^`);
	const version = `v${JSON.parse(git('show', `${ref}:package.json`)).version}`;
	// Before the version is bumped for it, the next release has no name yet.
	const tag = version === previous ? 'the next release' : version;
	const log = commits(previous, ref);
	if (log.length === 0) throw new Error(`no commits between ${previous} and ${ref}`);

	const client = new Anthropic();
	const message = await client.beta.messages
		.stream({
			model: MODEL,
			max_tokens: 16000,
			output_config: { effort: 'high' },
			// A request its safety classifiers decline goes to the model Anthropic recommends.
			betas: ['server-side-fallback-2026-07-01'],
			fallbacks: 'default',
			system: SYSTEM,
			messages: [
				{
					role: 'user',
					content: `<release>${tag}</release>\n<previous>${previous}</previous>\n<commits>\n${log
						.map((commit) => `<commit>\n${commit}\n</commit>`)
						.join('\n')}\n</commits>`
				}
			]
		})
		.finalMessage();

	if (message.stop_reason === 'refusal') {
		throw new Error(`the model declined (${message.stop_details?.category ?? 'no category'})`);
	}
	if (message.stop_reason === 'max_tokens') throw new Error('the notes ran past max_tokens');
	const notes = message.content
		.filter((block) => block.type === 'text')
		.map((block) => block.text)
		.join('')
		.trim();
	if (!notes.includes("## What's new")) throw new Error(`unexpected answer:\n${notes}`);
	process.stdout.write(`${notes}\n`);
	console.error(
		`Notes for ${tag} from ${log.length} commits since ${previous}, by ${message.model}: ${message.usage.input_tokens} tokens in, ${message.usage.output_tokens} out.`
	);
}

main().catch((err) => {
	if (err instanceof Anthropic.AuthenticationError) {
		console.error('ANTHROPIC_API_KEY was refused: is it set, and still valid?');
	} else if (err instanceof Anthropic.RateLimitError) {
		console.error(`Rate limited or out of credit: ${err.message}`);
	} else if (err instanceof Anthropic.APIConnectionError) {
		console.error(`Couldn't reach the Anthropic API: ${err.message}`);
	} else if (err instanceof Anthropic.APIError) {
		console.error(`The Anthropic API answered ${err.status}: ${err.message}`);
	} else {
		console.error(err instanceof Error ? err.message : String(err));
	}
	process.exitCode = 1;
});
