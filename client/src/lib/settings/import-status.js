// Import (#74) status copy and reply reading, shared by the Data page and
// the API client so both can be tested without a browser.

/**
 * A short human byte count: `512 B`, `2 KB`, `1.5 MB`.
 * @param {number} bytes
 * @returns {string}
 */
export function formatBytes(bytes) {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

const RESTORING =
	'Restoring… the server is checking the archive and rebuilding the search index. This can take a while for a large archive.';

/**
 * The one status line while an import runs. `uploaded` is what the request
 * reported, not a comparison of the numbers: before the first progress event
 * nothing is known about the body (an empty file has a total of 0), and the
 * line says Uploading until the browser has sent it all.
 * @param {{ importing: boolean, sent: number, total: number, uploaded: boolean }} state
 * @returns {string}
 */
export function importStatusText({ importing, sent, total, uploaded }) {
	if (!importing) return '';
	if (uploaded) return RESTORING;
	if (total > 0) return `Uploading… ${formatBytes(sent)} of ${formatBytes(total)}`;
	return 'Uploading…';
}

/**
 * Read the import route's reply. Only a 2xx carrying the route's own
 * `ok: true` is a restore; anything else (a proxy's page, an empty body) is
 * reported as unexpected rather than shown as one. An error keeps the
 * server's code and message; a body that is not JSON is shown as it came,
 * and an empty message becomes a fixed phrase, so the message is never
 * empty.
 * @typedef {{ ok: true, outcome: Record<string, unknown> }} ImportOk
 * @typedef {{ ok: false, code: string, message: string }} ImportRefused
 * @param {number} status
 * @param {string} text
 * @returns {ImportOk | ImportRefused}
 */
export function importReply(status, text) {
	/** @type {Record<string, unknown> | null} */
	let body = null;
	try {
		const parsed = JSON.parse(text);
		if (typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) body = parsed;
	} catch {
		body = null;
	}
	if (status >= 200 && status < 300) {
		if (body && body.ok === true) return { ok: true, outcome: body };
		return {
			ok: false,
			code: 'unexpected_reply',
			message: `unexpected reply from the server (HTTP ${status}); cannot tell whether the companion was restored, check the server log`,
		};
	}
	const code = typeof body?.error === 'string' && body.error ? body.error : `http_${status}`;
	// A JSON error carries its own message; raw text is shown only when the
	// body was not JSON at all.
	const message = body
		? (typeof body.message === 'string' && body.message) || 'import failed'
		: text || 'import failed';
	return { ok: false, code, message };
}
