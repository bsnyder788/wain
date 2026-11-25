/**
 * Reading a wall's photo with the engine's image analysis: finding the
 * wall's outline, and what is on the wall. Everything runs on this device;
 * the photo is not sent anywhere.
 *
 * The engine does the looking (see `crates/wain-vision`). This module gets
 * the photo's pixels to it and tidies what comes back for the project.
 */
import {
	findObstructions,
	findWall,
	type Length,
	type Obstruction,
	type Pixels,
	type Rect,
	type UnitPrefs,
	type Wall,
	type WallOutline
} from '#lib/engine/index.ts';
import { loadPhoto } from '#lib/photos.ts';

/**
 * Longest side of the copy that is analysed. Enough to make out a cover
 * plate across a room, and small enough to take a fraction of a second.
 */
const MAX_SIDE = 1024;

/** Decodes a stored photo into pixels, shrunk to at most `MAX_SIDE`. */
export async function decodePixels(blob: Blob): Promise<Pixels> {
	const bitmap = await createImageBitmap(blob);
	try {
		const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
		const width = Math.max(1, Math.round(bitmap.width * scale));
		const height = Math.max(1, Math.round(bitmap.height * scale));
		const canvas = document.createElement('canvas');
		canvas.width = width;
		canvas.height = height;
		const context = canvas.getContext('2d', { willReadFrequently: true });
		if (!context) throw new Error('This browser cannot process images.');
		context.drawImage(bitmap, 0, 0, width, height);
		return context.getImageData(0, 0, width, height);
	} finally {
		bitmap.close();
	}
}

// The last photo decoded. Finding corners and then finding what is on the
// wall both need the same pixels, usually seconds apart.
let cached: { id: string; pixels: Promise<Pixels> } | undefined;

/** The pixels of a stored photo, or `undefined` if it is no longer stored. */
async function pixelsOf(imageId: string): Promise<Pixels | undefined> {
	if (cached?.id !== imageId) {
		const pixels = loadPhoto(imageId).then((blob) => {
			if (!blob) throw new Error('missing');
			return decodePixels(blob);
		});
		cached = { id: imageId, pixels };
	}
	try {
		return await cached.pixels;
	} catch {
		cached = undefined;
		return undefined;
	}
}

/**
 * Looks for the wall's outline in its photo. `undefined` when none stands
 * out or the photo cannot be read; the caller keeps whatever corners it had.
 */
export async function detectOutline(wall: Wall): Promise<WallOutline | undefined> {
	const id = wall.photo?.image_id;
	const pixels = id ? await pixelsOf(id) : undefined;
	return pixels ? findWall(pixels) : undefined;
}

/**
 * Looks for doors, windows, outlets and switches in the wall's photo and
 * returns those that are new: found things that overlap something already
 * on the wall are left out. Sizes are rounded to `grid`, since a photo is
 * only good for an estimate.
 */
export async function detectObstructions(wall: Wall, grid: Length): Promise<Obstruction[]> {
	const id = wall.photo?.image_id;
	const pixels = id ? await pixelsOf(id) : undefined;
	if (!pixels) return [];
	return tidy(findObstructions(wall, pixels), wall, grid);
}

/** Snaps found obstructions to the grid and drops those already on the wall. */
export function tidy(found: Obstruction[], wall: Wall, grid: Length): Obstruction[] {
	const fresh: Obstruction[] = [];
	for (const { kind, rect } of found) {
		const snapped = snap(rect, wall, grid);
		if (!snapped) continue;
		const known = [...wall.obstructions, ...fresh];
		if (known.some((other) => overlaps(other.rect, snapped))) continue;
		fresh.push({ kind, rect: snapped });
	}
	return fresh;
}

function snap(rect: Rect, wall: Wall, grid: Length): Rect | undefined {
	const round = (value: number, limit: number) =>
		Math.min(limit, Math.max(0, Math.round(value / grid) * grid));
	const x = round(rect.x, wall.width);
	const y = round(rect.y, wall.height);
	const width = round(rect.x + rect.width, wall.width) - x;
	const height = round(rect.y + rect.height, wall.height) - y;
	return width > 0 && height > 0 ? { x, y, width, height } : undefined;
}

/** True when two rectangles share interior area. */
function overlaps(a: Rect, b: Rect): boolean {
	return (
		a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
	);
}

/** The grid that sizes read off a photo are rounded to: 1/4" or 5 mm. */
export function photoGrid(units: UnitPrefs): Length {
	return units.system === 'imperial' ? 6_350_000 : 5_000_000;
}
