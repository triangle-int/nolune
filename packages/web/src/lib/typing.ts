/** How often the page says again that someone is still typing: well within the server's TYPING_TTL_MS. */
export const TYPING_REFRESH_MS = 3_000;
/** Quiet this long after the last keystroke, someone has stopped typing, whatever the box holds. */
export const TYPING_IDLE_MS = 5_000;

/**
 * Tells the others in a chat whether this person is writing in its composer: that they are as they
 * type, again every TYPING_REFRESH_MS while they go on, and that they stopped when the box is
 * emptied, after TYPING_IDLE_MS of quiet, and when they leave. Sending the message stops it on the
 * server by itself. Reports go out one at a time, in order, so a "typing" still on its way never
 * lands after "stopped" or after the message (`settled`). A report that fails is dropped: the
 * server forgets someone who stopped saying they're typing.
 */
export class TypingReporter {
	readonly #report: (typing: boolean) => Promise<unknown>;
	/** When it last said they're typing, or null when it says they aren't. */
	#announcedAt: number | null = null;
	#idle: ReturnType<typeof setTimeout> | undefined;
	#queue: Promise<unknown> = Promise.resolve();

	constructor(report: (typing: boolean) => Promise<unknown>) {
		this.#report = report;
	}

	/** They changed what's in the box, which now holds `text`. */
	input(text: string): void {
		if (!text.trim()) return this.stop();
		clearTimeout(this.#idle);
		this.#idle = setTimeout(() => this.stop(), TYPING_IDLE_MS);
		const now = Date.now();
		if (this.#announcedAt !== null && now - this.#announcedAt < TYPING_REFRESH_MS) return;
		this.#announcedAt = now;
		this.#send(true);
	}

	/** They stopped typing: emptied the box, went quiet, or left. */
	stop(): void {
		clearTimeout(this.#idle);
		if (this.#announcedAt === null) return;
		this.#announcedAt = null;
		this.#send(false);
	}

	/** Their message went out, which stopped it on the server already. */
	sent(): void {
		clearTimeout(this.#idle);
		this.#announcedAt = null;
	}

	/** Resolves once every report so far has arrived, so a message sent next comes after them. */
	async settled(): Promise<void> {
		await this.#queue;
	}

	#send(typing: boolean): void {
		this.#queue = this.#queue.then(() => this.#report(typing)).catch(() => {});
	}
}
