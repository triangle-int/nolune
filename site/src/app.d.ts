// See https://svelte.dev/docs/kit/types#app.d.ts
declare global {
	namespace App {}

	/** A preview (Vercel's, for a pull request) or the dev server: vite.config.ts. */
	const __PREVIEW__: boolean;
}

export {};
