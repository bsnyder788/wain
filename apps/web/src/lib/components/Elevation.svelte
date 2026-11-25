<script lang="ts">
	import {
		formatLength,
		type Design,
		type Length,
		type UnitPrefs,
		type Wall,
		type WallLayout
	} from '#lib/engine/index.ts';
	import { OBSTRUCTION_NAMES } from '#lib/labels.ts';

	type Props = {
		wall: Wall;
		layout: WallLayout | undefined;
		design: Design;
		units: UnitPrefs;
	};
	let { wall, layout, design, units }: Props = $props();

	// The drawing is in millimetres with the origin at the top-left of the
	// wall; the engine's origin is bottom-left, so y is flipped here.
	const mm = (length: Length) => length / 1_000_000;
	const width = $derived(Math.max(mm(wall.width), 1));
	const height = $derived(Math.max(mm(wall.height), 1));
	const flip = (y: Length, h: Length) => height - mm(y) - mm(h);

	const text = $derived(Math.max(width, height) / 48);
	const margin = $derived(text * 3);
	const fmt = (length: Length) => formatLength(length, units);

	// Where another wall's boards meet this one, they are seen edge-on as a
	// strip one board thick: inside the wall end at an inside corner, and just
	// past it where this wall stops behind them at an outside corner.
	const neighbours = $derived.by(() => {
		const style = design.style;
		if (!layout || style.kind === 'picture_frame') return [];
		const t = mm(style.thickness);
		const strips: { x: number; side: string }[] = [];
		if (layout.left_joint.startsWith('inside')) strips.push({ x: 0, side: 'left' });
		if (layout.left_joint === 'outside_butt') strips.push({ x: -t, side: 'left' });
		if (layout.right_joint.startsWith('inside')) strips.push({ x: width - t, side: 'right' });
		if (layout.right_joint === 'outside_butt') strips.push({ x: width, side: 'right' });
		const bottom = mm(design.baseboard.height);
		const top = mm(style.height);
		return strips.map((strip) => ({ ...strip, y: height - top, width: t, height: top - bottom }));
	});

	// One label per distinct part per wall keeps the drawing readable.
	const labelled = $derived.by(() => {
		const seen = new Set<string>();
		return (layout?.parts ?? []).filter((part) => {
			if (seen.has(part.label)) return false;
			seen.add(part.label);
			return true;
		});
	});
</script>

<svg
	viewBox="{-margin} {-margin} {width + margin * 2} {height + margin * 2}"
	role="img"
	aria-label="Elevation drawing of {wall.name}"
>
	<rect class="wall" x="0" y="0" {width} {height} />

	{#if design.baseboard.existing && design.baseboard.height > 0}
		<rect
			class="existing"
			x="0"
			y={flip(0, design.baseboard.height)}
			{width}
			height={mm(design.baseboard.height)}
		/>
	{/if}

	{#each layout?.parts ?? [] as part, index (index)}
		<rect
			class="part"
			x={mm(part.rect.x)}
			y={flip(part.rect.y, part.rect.height)}
			width={mm(part.rect.width)}
			height={mm(part.rect.height)}
		/>
	{/each}

	{#each neighbours as strip (strip.side)}
		<rect class="neighbour" x={strip.x} y={strip.y} width={strip.width} height={strip.height}>
			<title>Edge of the adjoining wall's trim</title>
		</rect>
	{/each}

	{#each wall.obstructions as obstruction, index (index)}
		{@const rect = obstruction.rect}
		<rect
			class="obstruction {obstruction.kind}"
			x={mm(rect.x)}
			y={flip(rect.y, rect.height)}
			width={mm(rect.width)}
			height={mm(rect.height)}
		/>
		{#if mm(rect.width) > text * 5}
			<text
				class="obstruction-label"
				x={mm(rect.x) + mm(rect.width) / 2}
				y={flip(rect.y, rect.height) + mm(rect.height) / 2}
				font-size={text}
			>
				{OBSTRUCTION_NAMES[obstruction.kind]}
			</text>
		{/if}
	{/each}

	{#each labelled as part (part.label)}
		<text
			class="part-label"
			x={mm(part.rect.x) + mm(part.rect.width) / 2}
			y={flip(part.rect.y, part.rect.height) + mm(part.rect.height) / 2}
			font-size={text}
		>
			{part.label}
		</text>
	{/each}

	<line class="floor" x1={-margin / 2} y1={height} x2={width + margin / 2} y2={height} />
	<text class="dimension" x={width / 2} y={height + text * 1.8} font-size={text}>
		{fmt(wall.width)}
	</text>
	<text
		class="dimension"
		x={-text * 1.2}
		y={height / 2}
		font-size={text}
		transform="rotate(-90 {-text * 1.2} {height / 2})"
	>
		{fmt(wall.height)}
	</text>
</svg>

<style>
	svg {
		display: block;
		width: 100%;
		height: auto;
		max-height: 70vh;
	}
	rect,
	line {
		vector-effect: non-scaling-stroke;
	}
	.wall {
		fill: var(--wall);
		stroke: var(--line);
		stroke-width: 1;
	}
	.existing {
		fill: var(--trim);
		stroke: var(--line-soft);
		stroke-width: 1;
		stroke-dasharray: 4 3;
	}
	.part {
		fill: var(--trim);
		stroke: var(--line);
		stroke-width: 1;
	}
	.neighbour {
		fill: var(--line-soft);
		stroke: var(--line);
		stroke-width: 1;
	}
	.obstruction {
		fill: var(--obstruction);
		stroke: var(--line);
		stroke-width: 1;
	}
	.obstruction.window {
		fill: var(--glass);
	}
	.floor {
		stroke: var(--line);
		stroke-width: 2;
	}
	text {
		text-anchor: middle;
		dominant-baseline: central;
		fill: var(--ink);
		font-family: inherit;
	}
	.part-label {
		font-weight: 700;
		fill: var(--accent);
		paint-order: stroke;
		stroke: var(--trim);
		stroke-width: 0.25em;
	}
	.obstruction-label,
	.dimension {
		fill: var(--ink-soft);
	}
</style>
