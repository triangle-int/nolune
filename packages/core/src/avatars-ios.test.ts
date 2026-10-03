import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { arcToCubics, iosAvatarSource, parseAvatarColors, withoutArcs } from './avatars-ios.ts';

const root = fileURLToPath(new URL('../../../', import.meta.url));

describe('the avatars for iOS', () => {
	it('are as avatars.ts and the colors make them: run `node scripts/ios-avatars.ts` if not', () => {
		const css = readFileSync(`${root}packages/web/src/routes/layout.css`, 'utf8');
		const swift = readFileSync(`${root}ios/Nolune/Avatars.swift`, 'utf8');
		expect(swift).toBe(iosAvatarSource(parseAvatarColors(css)));
	});

	it('draw arcs as curves that end where they did and pass through the arc', () => {
		// A half circle of radius 5 around (5, 0), from (0, 0) to (10, 0), the long way round.
		const curves = arcToCubics(0, 0, 5, 5, 0, false, true, 10, 0);
		expect(curves).toHaveLength(2);
		expect(curves[1].slice(4)).toEqual([10, 0]);
		// Its middle, where the first quarter ends, is at the top or bottom of the circle.
		expect(Math.abs(curves[0][4] - 5)).toBeLessThan(1e-9);
		expect(Math.abs(Math.abs(curves[0][5]) - 5)).toBeLessThan(1e-9);
		expect(withoutArcs('M0 0A5 5 0 0 1 10 0Z')).toMatch(/^M0 0C.*C.* 10 0Z$/);
		expect(() => withoutArcs('m0 0l1 1')).toThrow();
	});
});
