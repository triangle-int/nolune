import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: { runes: true },
			// Every page is prerendered. On Vercel, adapter-static writes Vercel's output format by itself.
			adapter: adapter()
		})
	]
});
