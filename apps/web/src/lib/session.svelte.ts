/**
 * The project being edited, kept in the browser until accounts exist.
 *
 * `session.project` is deeply reactive, so components bind straight to its
 * fields and everything derived from it (the plan, the drawing) follows.
 */
import {
	defaultDesign,
	normalizeProject,
	starterProject,
	type Project,
	type StyleKind,
	type UnitPrefs,
	type Wall
} from '#lib/engine/index.ts';
import { deletePhoto, importPhoto } from '#lib/photos.ts';

// Bump the version when the project shape changes; older saves are dropped.
const PROJECT_KEY = 'wain.project.v2';
const UNITS_KEY = 'wain.units.v1';

// Filled in by `startSession()`, which the root layout runs before any page
// renders, so pages can treat the project as always present.
export const session: { project: Project } = $state({ project: null as unknown as Project });

function read<T>(key: string): T | undefined {
	try {
		const raw = localStorage.getItem(key);
		return raw === null ? undefined : (JSON.parse(raw) as T);
	} catch {
		return undefined;
	}
}

function write(key: string, value: unknown) {
	try {
		localStorage.setItem(key, JSON.stringify(value));
	} catch {
		// Storage can be full or disabled; the session still works in memory.
	}
}

/** A first guess at units for a new visitor; they can change it in Settings. */
function unitsForLocale(): UnitPrefs {
	const imperial = /-(US|LR|MM)\b/i.test(navigator.language);
	return {
		system: imperial ? 'imperial' : 'metric',
		fraction_denominator: 16,
		feet_and_inches: false
	};
}

/** Restores the saved project, or starts a new one. Needs the engine loaded. */
export function startSession() {
	const units = read<UnitPrefs>(UNITS_KEY) ?? unitsForLocale();
	// A save from an older version may lack newer fields; the engine fills
	// them in, or rejects the save if it is not a project at all.
	const saved = normalizeProject(read<unknown>(PROJECT_KEY));
	session.project = saved ?? newProject(units);

	$effect.root(() => {
		$effect(() => write(PROJECT_KEY, session.project));
	});
}

function newProject(units: UnitPrefs): Project {
	return { ...starterProject(units.system), units };
}

export function resetProject() {
	for (const wall of session.project.walls) forgetPhoto(wall);
	session.project = newProject(session.project.units);
}

/** Deletes a wall's stored image. Failure only leaves an orphan behind. */
function forgetPhoto(wall: Wall) {
	const id = wall.photo?.image_id;
	if (id) deletePhoto(id).catch(() => {});
}

/**
 * Attaches a photo to a wall, replacing any it had. The corners start as an
 * inset rectangle. The photo view then looks for the wall's outline and
 * moves them onto it if it finds one; either way the user can drag them.
 */
export async function setPhoto(wall: Wall, file: Blob) {
	const stored = await importPhoto(file);
	forgetPhoto(wall);
	wall.photo = {
		...stored,
		corners: [
			{ x: 0.15, y: 0.15 },
			{ x: 0.85, y: 0.15 },
			{ x: 0.85, y: 0.85 },
			{ x: 0.15, y: 0.85 }
		]
	};
}

export function removePhoto(wall: Wall) {
	forgetPhoto(wall);
	wall.photo = null;
}

/**
 * Changes how lengths are shown. The geometry is untouched. Switching between
 * imperial and metric also swaps the stock lengths, kerf and cut precision
 * for that system's defaults, since lumber is sold differently in each.
 */
export function setUnits(units: UnitPrefs) {
	const project = session.project;
	if (units.system !== project.units.system) {
		project.stock = starterProject(units.system).stock;
	}
	project.units = units;
	write(UNITS_KEY, units);
}

/** Switches style, starting from that style's default dimensions. */
export function setStyle(kind: StyleKind) {
	const project = session.project;
	if (kind === project.design.style.kind) return;
	project.design.style = defaultDesign(kind, project.units.system).style;
}

let nextId = 0;

/**
 * Adds a wall after the last one. Walls go around the room in order, so the
 * previous wall is assumed to turn an inside corner into the new one; the
 * user can change that on the previous wall.
 */
export function addWall() {
	const { walls } = session.project;
	const last = walls[walls.length - 1];
	if (last.corner === 'none') last.corner = 'inside';
	walls.push({
		id: `wall-${Date.now().toString(36)}-${nextId++}`,
		name: `Wall ${walls.length + 1}`,
		width: last.width,
		height: last.height,
		obstructions: [],
		corner: 'none',
		photo: null
	});
	return walls.length - 1;
}

export function removeWall(index: number) {
	const { walls } = session.project;
	if (walls.length <= 1) return;
	forgetPhoto(walls[index]);
	walls.splice(index, 1);
}
