// Bundles the CLI (and @nolune/core) into dist/ for the npm package. Node won't run .ts files
// from inside node_modules, so the published package ships JavaScript.
import { rmSync } from 'node:fs';
import { build } from 'esbuild';

// Split: dist/cli.js holds only what asks the gateway to run a command, which is most runs; the
// rest of nolune is in chunks it loads when it runs the command itself. Old chunks go first.
rmSync('dist/chunks', { recursive: true, force: true });

await build({
	entryPoints: { cli: 'packages/cli/src/index.ts' },
	outdir: 'dist',
	chunkNames: 'chunks/[name]-[hash]',
	bundle: true,
	splitting: true,
	platform: 'node',
	format: 'esm',
	target: 'node22',
	// Native module: installed from npm as a dependency of nolune.
	external: ['better-sqlite3'],
	// Some bundled dependencies are CommonJS and call require().
	banner: {
		js: "import { createRequire as __noluneCreateRequire } from 'node:module'; const require = __noluneCreateRequire(import.meta.url);"
	},
	legalComments: 'none',
	logLevel: 'info'
});
