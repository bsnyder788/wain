/**
 * Applies the engine's wall-to-photo transform. The engine works the
 * transform out (see `wallHomography`); applying it is three lines of
 * arithmetic, done here so that drawing does not call into WebAssembly for
 * every point.
 */
import type { Homography, Length, Rect } from '#lib/engine/index.ts';

type Matrix = Homography['forward'];

function apply(m: Matrix, x: number, y: number): [number, number] {
	const w = m[6] * x + m[7] * y + m[8];
	return [(m[0] * x + m[1] * y + m[2]) / w, (m[3] * x + m[4] * y + m[5]) / w];
}

/** Where a position on the wall appears in the photo, in pixels. */
export function toPhoto(h: Homography, x: Length, y: Length): [number, number] {
	return apply(h.forward, x, y);
}

/**
 * The position on the wall, in nanometres, that a photo pixel shows. It can
 * lie outside the wall. `undefined` for a point on the wall plane's horizon.
 */
export function toWall(h: Homography, u: number, v: number): [Length, Length] | undefined {
	const [x, y] = apply(h.inverse, u, v);
	return Number.isFinite(x) && Number.isFinite(y) ? [x, y] : undefined;
}

/**
 * A rectangle on the wall as an SVG `points` list in photo pixels. Straight
 * lines stay straight in perspective, so its four corners are enough.
 */
export function rectPoints(h: Homography, rect: Rect): string {
	const { x, y, width, height } = rect;
	return [
		toPhoto(h, x, y),
		toPhoto(h, x + width, y),
		toPhoto(h, x + width, y + height),
		toPhoto(h, x, y + height)
	]
		.map(([u, v]) => `${u.toFixed(2)},${v.toFixed(2)}`)
		.join(' ');
}
