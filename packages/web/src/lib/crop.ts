/**
 * Cropping a profile picture to a square: how far it's zoomed and where it sits, in shares of the
 * square's side so it doesn't matter how big the square is drawn. At zoom 1 the picture's shorter
 * side just fills the square, and the picture always covers it.
 */
export interface Crop {
	zoom: number;
	/** Where the picture's middle is from the square's, in shares of the square's side. */
	x: number;
	y: number;
}

export const MAX_ZOOM = 5;
export const CENTERED: Crop = { zoom: 1, x: 0, y: 0 };

/** The crop moved and zoomed back to where the picture still covers the square. */
export function clampCrop(crop: Crop, width: number, height: number): Crop {
	const zoom = Math.min(MAX_ZOOM, Math.max(1, crop.zoom));
	const short = Math.min(width, height);
	// How far each way the picture reaches past the square.
	const [spareX, spareY] = [((width / short) * zoom - 1) / 2, ((height / short) * zoom - 1) / 2];
	const clamp = (v: number, max: number) => Math.min(max, Math.max(-max, v));
	return { zoom, x: clamp(crop.x, spareX), y: clamp(crop.y, spareY) };
}

/** Moved by (dx, dy), in shares of the square's side. */
export function panBy(crop: Crop, dx: number, dy: number, width: number, height: number): Crop {
	return clampCrop({ ...crop, x: crop.x + dx, y: crop.y + dy }, width, height);
}

/** Zoomed to `zoom` about the square's middle, which keeps showing the same spot. */
export function zoomTo(crop: Crop, zoom: number, width: number, height: number): Crop {
	const next = Math.min(MAX_ZOOM, Math.max(1, zoom));
	const k = next / crop.zoom;
	return clampCrop({ zoom: next, x: crop.x * k, y: crop.y * k }, width, height);
}

/** The square of the picture the crop shows, in the picture's pixels: what `drawImage` takes. */
export function sourceSquare(
	crop: Crop,
	width: number,
	height: number
): { sx: number; sy: number; size: number } {
	const size = Math.min(width, height) / crop.zoom;
	return {
		sx: width / 2 - (0.5 + crop.x) * size,
		sy: height / 2 - (0.5 + crop.y) * size,
		size
	};
}
