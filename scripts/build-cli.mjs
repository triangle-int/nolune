// Bundles the CLI (and @btw/core) into dist/cli.js for the npm package. Node won't run .ts files
// from inside node_modules, so the published package ships JavaScript.
import { build } from 'esbuild';

await build({
	entryPoints: ['packages/cli/src/index.ts'],
	outfile: 'dist/cli.js',
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
