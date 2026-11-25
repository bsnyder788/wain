/// <reference types="node" />
import { readFile } from 'node:fs/promises';
import { beforeAll, describe, expect, it } from 'vitest';

import { rectPoints, toPhoto, toWall } from '../perspective.ts';
import {
	defaultDesign,
	formatLength,
	loadEngine,
	normalizeProject,
	parseLength,
	planProject,
	starterProject,
	wallHomography,
	type UnitPrefs,
	type Wall
} from './index.ts';

const INCH = 25_400_000;
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

	it('fills in fields missing from an older save', () => {
		const project = starterProject('metric');
		delete (project.walls[0] as Partial<Wall>).photo;
		expect(normalizeProject(project)?.walls[0].photo).toBeNull();
		expect(normalizeProject({ not: 'a project' })).toBeUndefined();
		expect(normalizeProject(null)).toBeUndefined();
	});

	it('gives each style defaults in the requested units', () => {
		const design = defaultDesign('picture_frame', 'metric');
		expect(design.style).toMatchObject({ kind: 'picture_frame', chair_rail_height: 860_000_000 });
	});
});
