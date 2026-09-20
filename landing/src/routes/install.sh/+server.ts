import type { RequestHandler } from './$types.js';
import script from '../../../../scripts/install.sh?raw';

// Prerendered from the repository at build time, so `curl -fsSL
// https://nolune.dev/install.sh | bash` runs the script at the deployed commit.
export const prerender = true;

export const GET: RequestHandler = () =>
	new Response(script, {
		headers: { 'Content-Type': 'text/plain; charset=utf-8' },
	});
