import type { RequestHandler } from './$types.js';
import script from '../../../../scripts/uninstall.sh?raw';

// Prerendered from the repository at build time, so the served uninstaller is
// the one at the deployed commit.
export const prerender = true;

export const GET: RequestHandler = () =>
	new Response(script, {
		headers: { 'Content-Type': 'text/plain; charset=utf-8' },
	});
