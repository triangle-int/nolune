import type { IncomingHttpHeaders, OutgoingHttpHeaders } from 'node:http';
import type { IncomingHttpHeaders as Http2Headers } from 'node:http2';

/** Connection-specific headers, which HTTP/2 forbids and every proxy drops. */
const HOP_BY_HOP = new Set([
	'connection',
	'keep-alive',
	'proxy-connection',
	'transfer-encoding',
	'upgrade',
	'te',
	'trailer',
	'host',
	'http2-settings',
	'expect'
]);

/**
 * A request's or answer's headers as they go on to the next hop, HTTP/1.1 or HTTP/2: without the
 * connection-specific ones (and those the Connection header names) or HTTP/2's pseudo-headers.
 */
export function endToEnd(headers: IncomingHttpHeaders | Http2Headers): OutgoingHttpHeaders {
	const listed = new Set(
		String(headers.connection ?? '')
			.split(',')
			.map((token) => token.trim().toLowerCase())
	);
	const kept: OutgoingHttpHeaders = {};
	for (const [name, value] of Object.entries(headers)) {
		if (value === undefined || name.startsWith(':') || HOP_BY_HOP.has(name) || listed.has(name)) {
			continue;
		}
		kept[name] = value;
	}
	return kept;
}
