/** Display names for the engine's enums. */
import type { EndCut, Material, ObstructionKind, PartKind, StyleKind } from '#lib/engine/index.ts';

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
	miter: '45° miter'
};

export const OBSTRUCTION_NAMES: Record<ObstructionKind, string> = {
	door: 'Door',
	opening: 'Opening',
	window: 'Window',
	outlet: 'Outlet',
	switch: 'Switch',
	vent: 'Vent',
	other: 'Other'
};
