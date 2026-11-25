/// <reference types="node" />
import { readFile } from 'node:fs/promises';
import { beforeAll, describe, expect, it } from 'vitest';

import {
	defaultDesign,
	formatLength,
	loadEngine,
	parseLength,
	planProject,
	starterProject,
	type UnitPrefs
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

	it('gives each style defaults in the requested units', () => {
		const design = defaultDesign('picture_frame', 'metric');
		expect(design.style).toMatchObject({ kind: 'picture_frame', chair_rail_height: 860_000_000 });
	});
});
