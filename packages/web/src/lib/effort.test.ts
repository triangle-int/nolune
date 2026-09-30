import { describe, expect, it } from 'vitest';
import { FIRST_STOP, LAST_INSET, levelAt, stageOf } from './effort';

describe('levelAt', () => {
	const width = 258;
	const lastStop = width - LAST_INSET;

	it('puts the stops between the first and last stop', () => {
		expect(levelAt(FIRST_STOP, width, 4)).toBe(0);
		expect(levelAt(lastStop, width, 4)).toBe(4);
		expect(levelAt((FIRST_STOP + lastStop) / 2, width, 4)).toBe(2);
	});

	it('clamps past the ends', () => {
		expect(levelAt(0, width, 4)).toBe(0);
		expect(levelAt(width + 40, width, 4)).toBe(4);
	});

	it('is fractional between stops', () => {
		const quarter = FIRST_STOP + (lastStop - FIRST_STOP) / 8;
		expect(levelAt(quarter, width, 4)).toBeCloseTo(0.5);
	});

	it('stays at 0 with a single level or no room', () => {
		expect(levelAt(120, width, 0)).toBe(0);
		expect(levelAt(120, 40, 4)).toBe(0);
	});
});

describe('stageOf', () => {
	it('gives each of five levels its own stage', () => {
		expect([0, 1, 2, 3, 4].map((level) => stageOf(level, 4))).toEqual([0, 1, 2, 3, 4]);
	});

	it('ends on the supernova with fewer levels', () => {
		expect([0, 1, 2].map((level) => stageOf(level, 2))).toEqual([0, 2, 4]);
	});

	it('is the calm star with a single level', () => {
		expect(stageOf(0, 0)).toBe(0);
	});
});
