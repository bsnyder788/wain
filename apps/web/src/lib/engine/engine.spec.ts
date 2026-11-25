/// <reference types="node" />
import { readFile } from 'node:fs/promises';
import { beforeAll, describe, expect, it } from 'vitest';

import { photoGrid, tidy } from '../detect.ts';
import { rectPoints, toPhoto, toWall } from '../perspective.ts';
import { designFacts, levelLines, placements } from '../plan-sheet.ts';
import {
	defaultDesign,
	findObstructions,
	findWall,
	formatLength,
	loadEngine,
	normalizeProject,
	parseLength,
	planProject,
	starterProject,
	wallHomography,
	type Pixels,
	type UnitPrefs,
	type Wall
} from './index.ts';

const INCH = 25_400_000;
const MM = 1_000_000;
const imperial: UnitPrefs = { system: 'imperial', fraction_denominator: 16, feet_and_inches: false };

beforeAll(async () => {
	const wasm = await readFile(new URL('./pkg/wain_wasm_bg.wasm', import.meta.url));
	await loadEngine(wasm);
});

describe('engine', () => {
	it('plans the starter project', () => {
		const result = planProject(starterProject('imperial'));
		if (!('ok' in result)) throw new Error('expected a plan');
		expect(result.ok.walls).toHaveLength(1);
		expect(result.ok.cut_list[0]).toMatchObject({ label: 'A', kind: 'batten', quantity: 10 });
		expect(result.ok.cut_list[0].length).toBe(45.25 * INCH);
	});

	it('returns validation issues instead of throwing', () => {
		const project = starterProject('metric');
		project.walls[0].width = 0;
		const result = planProject(project);
		if (!('error' in result)) throw new Error('expected an error');
		expect(result.error.issues[0].path).toBe('walls[0].width');
	});

	it('parses and formats lengths in either system', () => {
		expect(parseLength('32 1/2', 'imperial')).toEqual({ ok: 32.5 * INCH });
		expect(parseLength('2\' 8 1/2"', 'metric')).toEqual({ ok: 32.5 * INCH });
		expect(parseLength('826', 'metric')).toEqual({ ok: 826_000_000 });
		expect(parseLength('nope', 'metric')).toHaveProperty('error');
		expect(formatLength(32.5 * INCH, imperial)).toBe('32 1/2"');
		expect(formatLength(32.5 * INCH, { ...imperial, system: 'metric' })).toBe('825.5 mm');
	});

	it('maps a wall onto its photo and back', () => {
		const wall = starterProject('imperial').walls[0];
		expect(wallHomography(wall)).toBeUndefined();

		// A 12' x 8' wall seen at an angle in a 1000 x 800 photo.
		wall.photo = {
			image_id: 'test',
			width: 1000,
			height: 800,
			corners: [
				{ x: 0.1, y: 0.1 },
				{ x: 0.9, y: 0.275 },
				{ x: 0.88, y: 0.7 },
				{ x: 0.12, y: 0.875 }
			]
		};
		const result = wallHomography(wall);
		if (!result || !('ok' in result)) throw new Error('expected a transform');
		const h = result.ok;

		// The wall's top-left corner (x = 0, y = ceiling) is the first marked corner.
		const [u, v] = toPhoto(h, 0, wall.height);
		expect(u).toBeCloseTo(100, 6);
		expect(v).toBeCloseTo(80, 6);
		// The bottom-right marked corner is the far end of the wall at the floor.
		const back = toWall(h, 880, 560);
		expect(back?.[0]).toBeCloseTo(wall.width, -2);
		expect(Math.abs(back?.[1] ?? NaN)).toBeLessThan(100);
		// The whole wall as a polygon is the marked outline, from the bottom-left.
		expect(rectPoints(h, { x: 0, y: 0, width: wall.width, height: wall.height })).toBe(
			'120.00,700.00 880.00,560.00 900.00,220.00 100.00,80.00'
		);

		// Crossed corners are an error, not a crash.
		[wall.photo.corners[0], wall.photo.corners[1]] = [wall.photo.corners[1], wall.photo.corners[0]];
		expect(wallHomography(wall)).toHaveProperty('error');
	});

	it('finds the wall and a door in a photo', () => {
		const pixels = roomPhoto();
		const outline = findWall(pixels);
		if (!outline) throw new Error('expected an outline');
		expect(outline.sides).toEqual({ top: true, right: true, bottom: true, left: true });
		// The wall was drawn from (50, 40) to (350, 240) in a 400 x 300 photo.
		const expected = [
			[50, 40],
			[350, 40],
			[350, 240],
			[50, 240]
		];
		outline.corners.forEach((corner, index) => {
			expect(Math.abs(corner.x * 400 - expected[index][0])).toBeLessThan(3);
			expect(Math.abs(corner.y * 300 - expected[index][1])).toBeLessThan(3);
		});

		// A 3 m x 2.4 m wall, so the door drawn on it is 800 mm x 2040 mm,
		// 500 mm from the left.
		const wall = starterProject('metric').walls[0];
		expect(findObstructions(wall, pixels)).toEqual([]);
		wall.width = 3000 * MM;
		wall.height = 2400 * MM;
		wall.photo = { image_id: 'test', width: 400, height: 300, corners: outline.corners };
		const found = findObstructions(wall, pixels);
		expect(found).toHaveLength(1);
		expect(found[0].kind).toBe('door');
		const rect = found[0].rect;
		expect(rect.y).toBe(0);
		expect(Math.abs(rect.x - 500 * MM)).toBeLessThan(40 * MM);
		expect(Math.abs(rect.width - 800 * MM)).toBeLessThan(40 * MM);
		expect(Math.abs(rect.height - 2040 * MM)).toBeLessThan(40 * MM);

		// A featureless photo has no wall to find; mismatched pixels are an error.
		const blank = { data: new Uint8ClampedArray(400 * 300 * 4).fill(128), width: 400, height: 300 };
		expect(findWall(blank)).toBeUndefined();
		expect(() => findWall({ ...blank, width: 399 })).toThrow();
	});

	it('tidies what was found on a wall before offering it', () => {
		const wall = starterProject('metric').walls[0];
		wall.obstructions.push({
			kind: 'door',
			rect: { x: 400 * MM, y: 0, width: 900 * MM, height: 2080 * MM }
		});
		const metric: UnitPrefs = { ...imperial, system: 'metric' };
		const grid = photoGrid(metric);
		expect(grid).toBe(5 * MM);
		expect(photoGrid(imperial)).toBe(INCH / 4);

		const tidied = tidy(
			[
				// The door that is already on the wall, seen slightly differently.
				{ kind: 'door', rect: { x: 412 * MM, y: 0, width: 880 * MM, height: 2071 * MM } },
				{ kind: 'outlet', rect: { x: 2003.4 * MM, y: 298.2 * MM, width: 71 * MM, height: 117 * MM } },
				// Hanging off the end of the wall: trimmed to fit.
				{ kind: 'window', rect: { x: 3001 * MM, y: 900 * MM, width: 700 * MM, height: 1700 * MM } }
			],
			wall,
			grid
		);
		expect(tidied).toEqual([
			{ kind: 'outlet', rect: { x: 2005 * MM, y: 300 * MM, width: 70 * MM, height: 115 * MM } },
			{ kind: 'window', rect: { x: 3000 * MM, y: 900 * MM, width: 600 * MM, height: 1500 * MM } }
		]);
		// Everything offered is something the engine will accept.
		wall.obstructions.push(...tidied);
		expect(planProject({ ...starterProject('metric'), walls: [wall] })).toHaveProperty('ok');
	});

	it('fills in fields missing from an older save', () => {
		const project = starterProject('metric');
		delete (project.walls[0] as Partial<Wall>).photo;
		expect(normalizeProject(project)?.walls[0].photo).toBeNull();
		expect(normalizeProject({ not: 'a project' })).toBeUndefined();
		expect(normalizeProject(null)).toBeUndefined();
	});

	it('lists where each piece goes for the printed plan', () => {
		const project = starterProject('imperial');
		const result = planProject(project);
		if (!('ok' in result)) throw new Error('expected a plan');
		const rows = placements(result.ok.walls[0]);

		// The rail first, then ten battens left to right.
		expect(rows).toHaveLength(11);
		expect(rows[0]).toMatchObject({ label: 'B', piece: 'Top rail', fromLeft: 0, aboveFloor: 50.5 * INCH });
		expect(rows[1]).toMatchObject({ label: 'A', piece: 'Batten', fromLeft: 0, aboveFloor: 5.25 * INCH });
		expect(rows[10].fromLeft).toBe(141.5 * INCH);
		const lefts = rows.slice(1).map((row) => row.fromLeft);
		expect(lefts).toEqual([...lefts].sort((a, b) => a - b));

		// Level lines: the underside and the top of the rail.
		expect(levelLines(result.ok.walls[0])).toEqual([50.5 * INCH, 54 * INCH]);
		expect(designFacts(project.design, imperial)).toContainEqual(['Battens', '2 1/2" wide']);
	});

	it('lists picture-frame boxes as boxes, not four strips each', () => {
		const project = starterProject('imperial');
		project.design = defaultDesign('picture_frame', 'imperial');
		const result = planProject(project);
		if (!('ok' in result)) throw new Error('expected a plan');
		const rows = placements(result.ok.walls[0]);

		// One chair rail and five boxes, from 20 strips of moulding.
		expect(rows.map((row) => row.piece)).toEqual([
			'Chair rail',
			...Array(5).fill('Box of frame moulding')
		]);
		const box = rows[1];
		expect(box.label).toBe('B + C');
		expect(box.fromLeft).toBe(3.5 * INCH);
		expect(box.aboveFloor).toBe(8.75 * INCH);
		expect(box.height).toBe(19.25 * INCH);
		expect(box.width / INCH).toBeCloseTo(24.6, 5);
	});

	it('gives each style defaults in the requested units', () => {
		const design = defaultDesign('picture_frame', 'metric');
		expect(design.style).toMatchObject({ kind: 'picture_frame', chair_rail_height: 860_000_000 });
	});
});

/**
 * A 400 x 300 drawing of a room: a wall from (50, 40) to (350, 240) with a
 * cased door on it, a ceiling, a floor, and the neighbouring walls running
 * off at an angle the way they do in a photo.
 */
function roomPhoto(): Pixels {
	const [width, height] = [400, 300];
	const data = new Uint8ClampedArray(width * height * 4);
	const colour = (x: number, y: number): [number, number, number] => {
		// How far past the wall's ends this pixel is, if it is.
		const beside = Math.max(50 - x, x - 349, 0);
		const top = 40 - beside * 0.6;
		const bottom = 240 + beside * 0.8;
		if (y < top) return [238, 238, 234];
		if (y >= bottom) return [128, 92, 64];
		if (x < 50) return [173, 168, 156];
		if (x >= 350) return [220, 214, 199];
		if (x >= 100 && x < 180 && y >= 70) {
			const casing = x < 107 || x >= 173 || y < 77;
			return casing ? [244, 243, 238] : [120, 90, 70];
		}
		return [206, 200, 186];
	};
	for (let y = 0; y < height; y++) {
		for (let x = 0; x < width; x++) {
			data.set([...colour(x, y), 255], (y * width + x) * 4);
		}
	}
	return { data, width, height };
}
