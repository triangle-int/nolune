import { createIo, type Io } from '../io.ts';

/**
 * An io that records what a command prints, with the environment, folder and stdin a test gives
 * it. Without `stdin`, stdin is a terminal with nothing piped in.
 */
export function testIo(
	opts: { env?: Record<string, string>; cwd?: string; stdin?: string; signal?: AbortSignal } = {}
): { io: Io; out: () => string; err: () => string } {
	let out = '';
	let err = '';
	const io = createIo({
		stdout: (text) => (out += text),
		stderr: (text) => (err += text),
		env: opts.env ?? {},
		cwd: opts.cwd ?? '/nonexistent',
		stdinIsTTY: opts.stdin === undefined,
		readStdin: async () => opts.stdin ?? '',
		signal: opts.signal ?? new AbortController().signal
	});
	return { io, out: () => out, err: () => err };
}
