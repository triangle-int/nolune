import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig(({ command }) => ({
	define: {
		// Vercel's previews (a pull request's) and the dev server show what isn't open yet.
		__PREVIEW__: JSON.stringify(command === 'serve' || process.env.VERCEL_ENV === 'preview')
	},
	plugins: [
		sveltekit({
			compilerOptions: { runes: true },
			// Every page is prerendered. On Vercel, adapter-static writes Vercel's output format by itself.
			adapter: adapter(),
			prerender: {
				handleHttpError: ({ path, message }) => {
					// /docs is the docs site, which this domain serves from another project (vercel.json).
					if (path === '/docs' || path.startsWith('/docs/')) return;
					throw new Error(message);
				}
			}
		})
	]
}));
