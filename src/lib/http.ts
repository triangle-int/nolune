/**
 * What a failed request's response says, for people: the `message` SvelteKit's `error()` sends,
 * or SvelteKit's own plain text. Null when there's nothing to show: an empty body, or an HTML
 * page, which comes from whatever is in front of the gateway (the tunnel's or a proxy's error
 * page) and reads as markup. Callers then say the status.
 */
export function errorMessage(body: string, contentType: string | null): string | null {
	try {
		const data = JSON.parse(body) as { message?: unknown } | null;
		if (typeof data?.message === 'string') return data.message;
	} catch {
		// not JSON
	}
	const text = body.trim();
	if (!text || contentType?.toLowerCase().includes('text/html')) return null;
	return text;
}
