// Bundles the CLI (and @btw/core) into dist/cli.js for the npm package. Node won't run .ts files
// from inside node_modules, so the published package ships JavaScript.
import { rmSync } from 'node:fs';
import { build } from 'esbuild';

// Chunks from an earlier build would ship too.
rmSync('dist', { recursive: true, force: true });

await build({
	entryPoints: ['packages/cli/src/index.ts'],
	outdir: 'dist',
	entryNames: 'cli',
	// SDKs that core imports on first use (Claude Code's, OpenAI's) go in chunks of their own, so
	// every `btw` command doesn't parse them. Inline, the Agent SDK alone made each one ~65 ms slower.
	splitting: true,
	chunkNames: 'chunks/[name]-[hash]',
	bundle: true,
	platform: 'node',
	format: 'esm',
	target: 'node22',
	// Native module: installed from npm as a dependency of btw-agent.
	external: ['better-sqlite3'],
	// Some bundled dependencies are CommonJS and call require().
	banner: {
		js: "import { createRequire as __btwCreateRequire } from 'node:module'; const require = __btwCreateRequire(import.meta.url);"
	},
	legalComments: 'none',
	logLevel: 'info'
});
