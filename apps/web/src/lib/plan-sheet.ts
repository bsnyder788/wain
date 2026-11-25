/**
 * Turns a wall's layout into what a printed plan needs: where each piece
 * goes, measured from the wall's left end and from the floor.
 */
import type {
	Design,
	Length,
	Part,
	PartKind,
	UnitPrefs,
	WallLayout
} from '#lib/engine/index.ts';
import { formatLength } from '#lib/engine/index.ts';
import { PART_NAMES, STYLE_NAMES } from '#lib/labels.ts';

/** One line of a wall's placement table. */
export type Placement = {
	/** Cut-list labels of the pieces involved, e.g. `A`, or `B + C` for a box. */
	label: string;
	piece: string;
	/** Left edge, from the wall's left end. Negative past an outside corner. */
	fromLeft: Length;
	/** Bottom edge, above the floor. */
	aboveFloor: Length;
	width: Length;
	height: Length;
};

/** A part lying along the wall rather than standing up it. */
function isHorizontal(part: Part): boolean {
	return part.length === part.rect.width && part.rect.height === part.profile.width;
}

/** The order pieces go up in, which is the order the table lists them. */
const KIND_ORDER: PartKind[] = [
	'baseboard',
	'bottom_rail',
	'chair_rail',
	'top_rail',
	'stile',
	'batten',
	'frame_moulding'
];

/**
 * Where every piece goes on a wall. Rails and battens get a line each. The
 * four strips of a picture-frame box are listed together as one box, since
 * that is how it is marked out on the wall.
 */
export function placements(layout: WallLayout): Placement[] {
	const frames = layout.parts.filter((part) => part.kind === 'frame_moulding');
	const rows = layout.parts
		.filter((part) => part.kind !== 'frame_moulding')
		.sort(
			(a, b) =>
				KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind) ||
				a.rect.x - b.rect.x ||
				a.rect.y - b.rect.y
		)
		.map(
			(part): Placement => ({
				label: part.label,
				piece: PART_NAMES[part.kind],
				fromLeft: part.rect.x,
				aboveFloor: part.rect.y,
				width: part.rect.width,
				height: part.rect.height
			})
		);

	const boxes = boxesOf(frames);
	const boxRows = boxes.map(({ strips, uprights }): Placement => {
		const bottom = Math.min(...strips.map((strip) => strip.rect.y));
		const top = Math.max(...strips.map((strip) => strip.rect.y + strip.rect.height));
		const labels = [...new Set([...strips, ...uprights].map((part) => part.label))];
		return {
			label: labels.join(' + '),
			piece: 'Box of frame moulding',
			fromLeft: strips[0].rect.x,
			aboveFloor: bottom,
			width: strips[0].rect.width,
			height: top - bottom
		};
	});
	boxRows.sort((a, b) => a.fromLeft - b.fromLeft);

	return [...rows, ...boxRows];
}

type Box = { strips: Part[]; uprights: Part[] };

/**
 * Groups picture-frame moulding into boxes. A box's top and bottom strips
 * share its left edge and width, and its uprights stand within that span.
 */
function boxesOf(frames: Part[]): Box[] {
	const boxes = new Map<string, Box>();
	for (const strip of frames.filter(isHorizontal)) {
		const key = `${strip.rect.x}:${strip.rect.width}`;
		const box = boxes.get(key) ?? { strips: [], uprights: [] };
		box.strips.push(strip);
		boxes.set(key, box);
	}
	for (const upright of frames.filter((part) => !isHorizontal(part))) {
		for (const box of boxes.values()) {
			const first = box.strips[0].rect;
			if (upright.rect.x >= first.x && upright.rect.x < first.x + first.width) {
				box.uprights.push(upright);
			}
		}
	}
	return [...boxes.values()];
}

/**
 * Heights worth marking on a wall before anything goes up, lowest first: the
 * top and bottom edge of every rail, and the outside of every box.
 */
export function levelLines(layout: WallLayout): Length[] {
	const levels = new Set<Length>();
	const frames = layout.parts.filter((part) => part.kind === 'frame_moulding');
	for (const part of layout.parts.filter((part) => part.kind !== 'frame_moulding')) {
		if (!isHorizontal(part)) continue;
		levels.add(part.rect.y);
		levels.add(part.rect.y + part.rect.height);
	}
	for (const { strips } of boxesOf(frames)) {
		levels.add(Math.min(...strips.map((strip) => strip.rect.y)));
		levels.add(Math.max(...strips.map((strip) => strip.rect.y + strip.rect.height)));
	}
	levels.delete(0);
	return [...levels].sort((a, b) => a - b);
}

/** The design's dimensions as label and value pairs, for the plan's summary. */
export function designFacts(design: Design, units: UnitPrefs): [string, string][] {
	const fmt = (length: Length) => formatLength(length, units);
	const style = design.style;
	const facts: [string, string][] = [['Style', STYLE_NAMES[style.kind]]];

	switch (style.kind) {
		case 'picture_frame':
			facts.push(
				['Chair rail height', fmt(style.chair_rail_height)],
				['Chair rail', `${fmt(style.chair_rail_width)} wide, ${fmt(style.chair_rail_thickness)} thick`],
				['Panel moulding', `${fmt(style.moulding_width)} wide`],
				['Gap around boxes', fmt(style.gap)]
			);
			break;
		case 'board_and_batten':
			facts.push(
				['Height', fmt(style.height)],
				['Top rail', `${fmt(style.top_rail_width)} wide`],
				['Battens', `${fmt(style.batten_width)} wide`],
				['Board thickness', fmt(style.thickness)]
			);
			break;
		case 'shaker_panel':
			facts.push(
				['Height', fmt(style.height)],
				['Top rail', `${fmt(style.top_rail_width)} wide`],
				['Bottom rail', `${fmt(style.bottom_rail_width)} wide`],
				['Stiles', `${fmt(style.stile_width)} wide`],
				['Board thickness', fmt(style.thickness)]
			);
			break;
	}
	facts.push([
		'Baseboard',
		`${fmt(design.baseboard.height)} tall, ${design.baseboard.existing ? 'existing' : 'new'}`
	]);
	return facts;
}
