import { error, json } from '@sveltejs/kit';
import { MAX_PAYLOAD_BYTES, fireWebhook } from '@nolune/core';
import type { RequestHandler } from './$types';

/** Starts a run of the webhook trigger with this token. The request body goes to the agent. */
export const POST: RequestHandler = async ({ params, request }) => {
	const declared = Number(request.headers.get('content-length') ?? 0);
	if (declared > MAX_PAYLOAD_BYTES) error(413, 'Body too large (64 KB max)');
	const body = await request.text();
	if (Buffer.byteLength(body) > MAX_PAYLOAD_BYTES) error(413, 'Body too large (64 KB max)');

	const result = fireWebhook(params.token, body);
	if (result === 'not_found') error(404, 'Not found');
	if (result === 'paused') error(409, 'This trigger is paused');
	if (result === 'busy') error(429, 'Too many runs are waiting; try again later');
	return json({ ok: true, run: result.id }, { status: 202 });
};
