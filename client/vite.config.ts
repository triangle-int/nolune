import tailwindcss from "@tailwindcss/vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig, loadEnv } from "vite";

export default defineConfig(({ mode }) => {
	// scripts/dev.sh runs its server on NOLUNE_DEV_PORT so an installed Nolune
	// on the default port keeps working beside it.
	const api = `localhost:${loadEnv(mode, ".", "NOLUNE_DEV_").NOLUNE_DEV_PORT || 26559}`;
	return {
		plugins: [
			tailwindcss(),
			sveltekit(),
		],
		build: {
			target: "esnext",
		},
		server: {
			proxy: {
				"/api/ws": { target: `ws://${api}`, ws: true },
				"/api": `http://${api}`,
			},
		},
	};
});
