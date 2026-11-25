/**
 * The typed front door to the Rust layout engine.
 *
 * `pkg/` (the WebAssembly build) and `bindings/` (TypeScript types) are both
 * generated from the Rust crates by `npm run engine`. Structured values cross
 * the WebAssembly boundary as JSON; this module is the only place that knows
 * that. Every `Length` is a number of nanometres: use `formatLength` and
 * `parseLength` rather than doing unit maths in the UI.
 */
import init, * as wasm from './pkg/wain_wasm.js';
import type { InitInput } from './pkg/wain_wasm.js';

import type { Design } from './bindings/Design';
import type { Homography } from './bindings/Homography';
import type { Length } from './bindings/Length';
import type { Obstruction } from './bindings/Obstruction';
import type { Plan } from './bindings/Plan';
import type { PlanError } from './bindings/PlanError';
import type { Project } from './bindings/Project';
import type { StyleKind } from './bindings/StyleKind';
import type { UnitPrefs } from './bindings/UnitPrefs';
import type { UnitSystem } from './bindings/UnitSystem';
import type { Wall } from './bindings/Wall';
import type { WallOutline } from './bindings/WallOutline';

export type { Baseboard } from './bindings/Baseboard';
export type { BoardAndBatten } from './bindings/BoardAndBatten';
export type { Corner } from './bindings/Corner';
export type { CutItem } from './bindings/CutItem';
export type { Design } from './bindings/Design';
export type { EndCut } from './bindings/EndCut';
export type { Homography } from './bindings/Homography';
export type { Issue } from './bindings/Issue';
export type { Joint } from './bindings/Joint';
export type { Length } from './bindings/Length';
export type { Material } from './bindings/Material';
export type { Obstruction } from './bindings/Obstruction';
export type { ObstructionKind } from './bindings/ObstructionKind';
export type { Part } from './bindings/Part';
export type { PartKind } from './bindings/PartKind';
export type { PictureFrame } from './bindings/PictureFrame';
export type { Plan } from './bindings/Plan';
export type { PlanError } from './bindings/PlanError';
export type { Point } from './bindings/Point';
export type { Profile } from './bindings/Profile';
export type { Project } from './bindings/Project';
export type { Rect } from './bindings/Rect';
export type { ShakerPanel } from './bindings/ShakerPanel';
export type { SidesFound } from './bindings/SidesFound';
export type { StockBoard } from './bindings/StockBoard';
export type { StockGroup } from './bindings/StockGroup';
export type { StockOptions } from './bindings/StockOptions';
export type { Style } from './bindings/Style';
export type { StyleKind } from './bindings/StyleKind';
export type { UnitPrefs } from './bindings/UnitPrefs';
export type { UnitSystem } from './bindings/UnitSystem';
export type { Wall } from './bindings/Wall';
export type { WallLayout } from './bindings/WallLayout';
export type { WallOutline } from './bindings/WallOutline';
export type { WallPhoto } from './bindings/WallPhoto';
export type { Warning } from './bindings/Warning';

export type PlanResult = { ok: Plan } | { error: PlanError };
export type ParseResult = { ok: Length } | { error: string };
export type HomographyResult = { ok: Homography } | { error: string };

let ready: Promise<void> | undefined;

/**
 * Loads the engine. Must resolve before any other function here is called.
 * In the browser the `.wasm` file is fetched; tests pass its bytes instead.
 */
export function loadEngine(source?: InitInput): Promise<void> {
	ready ??= init(source === undefined ? undefined : { module_or_path: source }).then(() => {});
	return ready;
}

/** Lays out a project: placed parts, cut list, stock plan and warnings. */
export function planProject(project: Project): PlanResult {
	return JSON.parse(wasm.plan(JSON.stringify(project)));
}

/**
 * Checks that a value loaded from storage is a project and fills in any
 * fields added since it was saved. Returns `undefined` if it is not usable.
 */
export function normalizeProject(value: unknown): Project | undefined {
	try {
		return JSON.parse(wasm.normalize_project(JSON.stringify(value)));
	} catch {
		return undefined;
	}
}

/**
 * The transform between a wall and its photo, working in photo pixels.
 * `undefined` when the wall has no photo; an error when the marked corners
 * do not outline a wall (crossed over, say, while one is being dragged).
 */
export function wallHomography(wall: Wall): HomographyResult | undefined {
	const photo = wall.photo;
	if (!photo) return undefined;
	const corners = photo.corners.map((p) => ({ x: p.x * photo.width, y: p.y * photo.height }));
	try {
		return { ok: JSON.parse(wasm.wall_homography(wall.width, wall.height, JSON.stringify(corners))) };
	} catch (error) {
		return { error: error instanceof Error ? error.message : String(error) };
	}
}

/**
 * A photo's pixels as a canvas gives them: four bytes each (red, green,
 * blue, alpha), row by row from the top-left. An `ImageData` fits.
 */
export type Pixels = { data: Uint8ClampedArray | Uint8Array; width: number; height: number };

function bytes(pixels: Pixels): Uint8Array {
	const { data } = pixels;
	return data instanceof Uint8Array
		? data
		: new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
}

/**
 * Looks for the wall's outline in a photo: its four corners, and which of
 * its sides were actually seen. `undefined` when no outline stands out. A
 * best guess for the user to check, not a measurement.
 */
export function findWall(pixels: Pixels): WallOutline | undefined {
	return JSON.parse(wasm.find_wall(bytes(pixels), pixels.width, pixels.height)) ?? undefined;
}

/**
 * Looks for doors, windows, outlets and switches in a wall's photo, placed
 * in the wall's own measurements using the corners marked on it. The pixels
 * can be a smaller copy of the stored photo. Empty when the wall has no
 * photo or its corners do not outline a wall. Suggestions for the user to
 * confirm: anything rectangular can pass for a window.
 */
export function findObstructions(wall: Wall, pixels: Pixels): Obstruction[] {
	if (!wall.photo) return [];
	return JSON.parse(
		wasm.find_obstructions(
			bytes(pixels),
			pixels.width,
			pixels.height,
			wall.width,
			wall.height,
			JSON.stringify(wall.photo.corners)
		)
	);
}

export function starterProject(system: UnitSystem): Project {
	return JSON.parse(wasm.starter_project(system));
}

export function defaultDesign(style: StyleKind, system: UnitSystem): Design {
	return JSON.parse(wasm.default_design(style, system));
}

/**
 * Reads a length the user typed. A bare number is taken in `system`; an
 * explicit unit (`"`, `'`, `mm`, `cm`, `m`) always wins.
 */
export function parseLength(input: string, system: UnitSystem): ParseResult {
	try {
		return { ok: wasm.parse_length(input, system) };
	} catch (error) {
		return { error: error instanceof Error ? error.message : String(error) };
	}
}

export function formatLength(length: Length, prefs: UnitPrefs): string {
	return wasm.format_length(length, JSON.stringify(prefs));
}
