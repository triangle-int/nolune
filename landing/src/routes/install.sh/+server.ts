import type { RequestHandler } from './$types.js';
import script from '../../../../scripts/install.sh?raw';

// Prerendered from the repository at build time, so `curl -fsSL
// https://nolune.dev/install.sh | bash` runs the script at the deployed commit.
//
// No response headers are set here because none reach production: Vercel
// serves the prerendered file from static output and derives its Content-Type
// from the `.sh` extension (application/x-sh), so a browser visit downloads
// the script instead of displaying it. `curl | bash` is unaffected. Restoring
// `text/plain` is a `headers` entry in landing/vercel.json (#32, slice 2).
// Under `vite dev` the string body already defaults to text/plain.
export const prerender = true;

export const GET: RequestHandler = () => new Response(script);
