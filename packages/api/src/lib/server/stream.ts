import type { Usage } from './openrouter.ts';

/** What a stream from OpenRouter said, once it ended: its generation and, when it got there, its usage. */
export interface StreamEnd {
	id: string | null;
	usage: Usage | null;
	/** Whether it ran to its end, rather than being cut off by either side. */
	complete: boolean;
}

/**
 * Passes a server-sent event stream on unchanged, reading as it goes the generation's id (in every
 * chunk) and its usage (in the last). `onEnd` is called once: at the end, when the reader cancels
 * (the family's request went away, which cancels OpenRouter's too), or when it fails.
 */
export function tapStream(
	upstream: ReadableStream<Uint8Array>,
	onEnd: (end: StreamEnd) => void
): ReadableStream<Uint8Array> {
	const reader = upstream.getReader();
	const decoder = new TextDecoder();
	let pending = '';
	let id: string | null = null;
	let usage: Usage | null = null;
	let ended = false;

	const end = (complete: boolean) => {
		if (ended) return;
		ended = true;
		onEnd({ id, usage, complete });
	};

	const scan = (text: string) => {
		pending += text;
		const lines = pending.split('\n');
		pending = lines.pop() ?? '';
		for (const line of lines) {
			if (!line.startsWith('data:')) continue;
			const data = line.slice(5).trim();
			if (!data.startsWith('{')) continue;
			try {
				const chunk = JSON.parse(data) as { id?: unknown; usage?: Usage | null };
				if (typeof chunk.id === 'string') id = chunk.id;
				if (chunk.usage) usage = chunk.usage;
			} catch {
				// Not a chunk of ours to read; it goes on as it came.
			}
		}
	};

	return new ReadableStream<Uint8Array>({
		async pull(controller) {
			try {
				const { done, value } = await reader.read();
				if (done) {
					scan(decoder.decode() + '\n');
					end(true);
					controller.close();
					return;
				}
				scan(decoder.decode(value, { stream: true }));
				controller.enqueue(value);
			} catch (err) {
				end(false);
				controller.error(err);
			}
		},
		cancel(reason) {
			end(false);
			return reader.cancel(reason);
		}
	});
}
