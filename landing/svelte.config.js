import adapter from '@sveltejs/adapter-vercel';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	kit: {
		adapter: adapter({
			runtime: 'nodejs22.x',
		}),
		prerender: {
			// The whole site prerenders; a dangling link or hash is a build error.
			handleHttpError: 'fail',
			handleMissingId: 'fail',
		},
	}
};

export default config;
