/**
 * The project being edited, kept in the browser until accounts exist.
 *
 * `session.project` is deeply reactive, so components bind straight to its
 * fields and everything derived from it (the plan, the drawing) follows.
 */
import {
	defaultDesign,
	planProject,
	starterProject,
	type Project,
	type StyleKind,
	type UnitPrefs
} from '#lib/engine/index.ts';

const PROJECT_KEY = 'wain.project.v1';
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

/** True when the engine accepts the value as a project at all. */
function isProject(value: unknown): value is Project {
	try {
		planProject(value as Project);
		return true;
	} catch {
		return false;
	}
}

/** Restores the saved project, or starts a new one. Needs the engine loaded. */
export function startSession() {
	const units = read<UnitPrefs>(UNITS_KEY) ?? unitsForLocale();
	const saved = read<unknown>(PROJECT_KEY);
	session.project = isProject(saved) ? saved : newProject(units);

	$effect.root(() => {
		$effect(() => write(PROJECT_KEY, session.project));
	});
}

function newProject(units: UnitPrefs): Project {
	return { ...starterProject(units.system), units };
}

export function resetProject() {
	session.project = newProject(session.project.units);
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

export function addWall() {
	const { walls } = session.project;
	const last = walls[walls.length - 1];
	walls.push({
		id: `wall-${Date.now().toString(36)}-${nextId++}`,
		name: `Wall ${walls.length + 1}`,
		width: last.width,
		height: last.height,
		obstructions: []
	});
	return walls.length - 1;
}

export function removeWall(index: number) {
	const { walls } = session.project;
	if (walls.length > 1) walls.splice(index, 1);
}
