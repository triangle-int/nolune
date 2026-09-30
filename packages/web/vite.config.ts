import tailwindcss from '@tailwindcss/vite';
import adapter from '@sveltejs/adapter-node';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';

export default defineConfig({
	plugins: [
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},

			adapter: adapter()
		})
	],
	server: {
		watch: {
			// Claude Code worktrees live in .claude/worktrees. A `svelte-kit sync` in one rewrites its
			// tsconfig.json, which Vite treats as a tsconfig change and full-reloads every open tab.
			ignored: ['**/.claude/**']
		}
	},
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node',
		setupFiles: ['../core/src/test/setup.ts'],
		expect: { requireAssertions: true }
	}
});
