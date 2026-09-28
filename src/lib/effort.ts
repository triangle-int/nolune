/**
 * The reasoning slider's geometry (`EffortSlider.svelte`): the track runs out of the star at the
 * left of its window, and the thumb's stops sit between `FIRST_STOP` and `LAST_INSET` from the
 * right edge, in pixels.
 */
export const TRACK_START = 46;
export const FIRST_STOP = 64;
export const LAST_INSET = 26;

/** The star's life has five stages, one per level when there are five levels. */
export const STAGES = 5;

/**
 * Where the pointer is, as a level from 0 to `last`: fractional between stops, clamped at the
 * ends. `x` is in pixels from the window's left edge, `width` the window's width.
 */
export function levelAt(x: number, width: number, last: number): number {
	const run = width - FIRST_STOP - LAST_INSET;
	if (last <= 0 || run <= 0) return 0;
	return Math.min(last, Math.max(0, ((x - FIRST_STOP) / run) * last));
}

/** The star's stage for a level: the first level is the calm star, the last the supernova. */
export function stageOf(level: number, last: number): number {
	if (last <= 0) return 0;
	return Math.round((level / last) * (STAGES - 1));
}
