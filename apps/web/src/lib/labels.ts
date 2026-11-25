/** Display names for the engine's enums. */
import type {
	Corner,
	EndCut,
	Joint,
	Material,
	ObstructionKind,
	PartKind,
	StyleKind
} from '#lib/engine/index.ts';

export const STYLE_NAMES: Record<StyleKind, string> = {
	picture_frame: 'Picture frame',
	board_and_batten: 'Board and batten',
	shaker_panel: 'Shaker panel'
};

export const STYLE_BLURBS: Record<StyleKind, string> = {
	picture_frame: 'A chair rail with mitered boxes of moulding beneath it.',
	board_and_batten: 'Vertical battens under a top rail.',
	shaker_panel: 'Flat stiles and rails that frame recessed panels.'
};

export const PART_NAMES: Record<PartKind, string> = {
	baseboard: 'Baseboard',
	chair_rail: 'Chair rail',
	top_rail: 'Top rail',
	bottom_rail: 'Bottom rail',
	stile: 'Stile',
	batten: 'Batten',
	frame_moulding: 'Frame moulding'
};

export const MATERIAL_NAMES: Record<Material, string> = {
	baseboard: 'Baseboard',
	board: 'Board',
	chair_rail: 'Chair rail',
	panel_moulding: 'Panel moulding'
};

export const END_CUT_NAMES: Record<EndCut, string> = {
	square: 'Square',
	frame_miter: '45° miter',
	outside_miter: 'Outside miter',
	cope: 'Cope'
};

/** The cuts at a piece's two ends, e.g. "Square" or "Cope / Outside miter". */
export function endCuts([first, second]: [EndCut, EndCut]): string {
	if (first !== second) return `${END_CUT_NAMES[first]} / ${END_CUT_NAMES[second]}`;
	return first === 'square' ? 'Square' : `${END_CUT_NAMES[first]}, both ends`;
}

export const CORNER_NAMES: Record<Corner, (next: string) => string> = {
	none: () => 'Trim ends here',
	inside: (next) => `Inside corner into ${next}`,
	outside: (next) => `Outside corner around to ${next}`
};

/**
 * What happens to this wall's trim where it meets `other`. Flat boards
 * overlap at corners; profiled mouldings are coped or mitered instead (a new
 * baseboard gets the same treatment as the chair rail).
 */
export function jointNote(joint: Joint, other: string, boards: boolean): string | undefined {
	switch (joint) {
		case 'free':
			return undefined;
		case 'inside_through':
			return boards
				? `Runs into the corner. ${other}'s trim butts against it, so the corner piece is cut wider to show the same face.`
				: `The chair rail runs square into the corner.`;
		case 'inside_butt':
			return boards
				? `Stops one board thickness short of the corner, against ${other}'s trim.`
				: `The chair rail is coped over ${other}'s.`;
		case 'outside_through':
			return boards
				? `Runs past the corner by one board thickness to cover the edge of ${other}'s trim. Rails are mitered.`
				: `The chair rail is mitered around the corner with ${other}'s.`;
		case 'outside_butt':
			return boards
				? `Stops at the corner behind the edge of ${other}'s trim, so the corner piece is cut narrower. Rails are mitered.`
				: `The chair rail is mitered around the corner with ${other}'s.`;
	}
}

export const OBSTRUCTION_NAMES: Record<ObstructionKind, string> = {
	door: 'Door',
	opening: 'Opening',
	window: 'Window',
	outlet: 'Outlet',
	switch: 'Switch',
	vent: 'Vent',
	other: 'Other'
};
