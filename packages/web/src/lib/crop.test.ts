import { describe, expect, it } from 'vitest';
import { CENTERED, MAX_ZOOM, clampCrop, panBy, sourceSquare, zoomTo } from './crop';

describe('cropping to a square', () => {
	it('starts on the middle of the picture, its shorter side filling the square', () => {
		expect(sourceSquare(CENTERED, 400, 200)).toEqual({ sx: 100, sy: 0, size: 200 });
		expect(sourceSquare(CENTERED, 300, 600)).toEqual({ sx: 0, sy: 150, size: 300 });
	});

	it('moves only as far as the picture still covers the square', () => {
		// A wide picture reaches half a square past each side, and not at all up or down.
		const right = panBy(CENTERED, 2, 1, 400, 200);
		expect(right).toEqual({ zoom: 1, x: 0.5, y: 0 });
		expect(sourceSquare(right, 400, 200)).toEqual({ sx: 0, sy: 0, size: 200 });
		expect(sourceSquare(panBy(CENTERED, -2, 0, 400, 200), 400, 200).sx).toBe(200);
	});

	it('zooms about the middle, keeping the same spot there', () => {
		const moved = panBy(zoomTo(CENTERED, 2, 200, 200), 0.25, 0, 200, 200);
		const middle = (crop: typeof moved) => {
			const { sx, size } = sourceSquare(crop, 200, 200);
			return sx + size / 2;
		};
		expect(middle(zoomTo(moved, 4, 200, 200))).toBeCloseTo(middle(moved));
		// Zooming back out pulls the picture back over the square.
		expect(zoomTo(moved, 1, 200, 200)).toEqual(CENTERED);
	});

	it('keeps the zoom between the picture filling the square and MAX_ZOOM', () => {
		expect(clampCrop({ zoom: 0.2, x: 0, y: 0 }, 100, 100).zoom).toBe(1);
		expect(zoomTo(CENTERED, 50, 100, 100).zoom).toBe(MAX_ZOOM);
	});
});
