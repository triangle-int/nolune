import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TYPING_IDLE_MS, TYPING_REFRESH_MS, TypingReporter } from './typing';

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(() => {
	vi.useRealTimers();
});

/** A reporter that writes down what it reports. */
function reporter() {
	const reports: boolean[] = [];
	const typing = new TypingReporter(async (report) => {
		reports.push(report);
	});
	return { typing, reports };
}

describe('TypingReporter', () => {
	it('says they are typing as they start, again every few seconds, and stopped once they go quiet', async () => {
		const { typing, reports } = reporter();
		typing.input('H');
		typing.input('He');
		await vi.advanceTimersByTimeAsync(TYPING_REFRESH_MS);
		typing.input('Hel');
		typing.input('Hell');
		await vi.advanceTimersByTimeAsync(TYPING_IDLE_MS - 1);
		await typing.settled();
		expect(reports).toEqual([true, true]);

		await vi.advanceTimersByTimeAsync(1);
		await typing.settled();
		expect(reports).toEqual([true, true, false]);
	});

	it('stops when the box is emptied, and says so once', async () => {
		const { typing, reports } = reporter();
		typing.input('Hi');
		typing.input('');
		typing.input('  ');
		typing.stop();
		await vi.advanceTimersByTimeAsync(TYPING_IDLE_MS);
		await typing.settled();
		expect(reports).toEqual([true, false]);
	});

	it('leaves stopping to the server once the message went out, and starts afresh', async () => {
		const { typing, reports } = reporter();
		typing.input('Hi');
		typing.sent();
		typing.stop();
		await vi.advanceTimersByTimeAsync(TYPING_IDLE_MS);
		await typing.settled();
		expect(reports).toEqual([true]);

		typing.input('A');
		await typing.settled();
		expect(reports).toEqual([true, true]);
	});

	it('sends one report at a time, in order', async () => {
		const started: boolean[] = [];
		let arrive = () => {};
		const typing = new TypingReporter((report) => {
			started.push(report);
			return report ? new Promise<void>((resolve) => (arrive = resolve)) : Promise.resolve();
		});
		typing.input('a');
		typing.input('');
		let settled = false;
		void typing.settled().then(() => (settled = true));
		await vi.advanceTimersByTimeAsync(0);
		expect(started).toEqual([true]);
		expect(settled).toBe(false);

		arrive();
		await typing.settled();
		expect(started).toEqual([true, false]);
	});

	it('goes on after a report that failed', async () => {
		const reports: boolean[] = [];
		const typing = new TypingReporter(async (report) => {
			reports.push(report);
			if (report) throw new TypeError('Failed to fetch');
		});
		typing.input('a');
		typing.stop();
		await typing.settled();
		expect(reports).toEqual([true, false]);
	});
});
