<script lang="ts">
	import type { Design, StyleKind, UnitPrefs } from '#lib/engine/index.ts';
	import { STYLE_BLURBS, STYLE_NAMES } from '#lib/labels.ts';
	import { setStyle } from '#lib/session.svelte.ts';
	import LengthInput from './LengthInput.svelte';

	type Props = { design: Design; units: UnitPrefs };
	let { design = $bindable(), units }: Props = $props();

	const KINDS = Object.keys(STYLE_NAMES) as StyleKind[];
	const style = $derived(design.style);
</script>

<div class="styles" role="radiogroup" aria-label="Style">
	{#each KINDS as kind (kind)}
		<button
			type="button"
			role="radio"
			aria-checked={style.kind === kind}
			class:selected={style.kind === kind}
			onclick={() => setStyle(kind)}
		>
			<strong>{STYLE_NAMES[kind]}</strong>
			<span>{STYLE_BLURBS[kind]}</span>
		</button>
	{/each}
</div>

<div class="fields">
	{#if style.kind === 'picture_frame'}
		<LengthInput
			label="Chair rail height"
			bind:value={style.chair_rail_height}
			{units}
			hint="Floor to top of rail"
		/>
		<LengthInput label="Chair rail width" bind:value={style.chair_rail_width} {units} />
		<LengthInput label="Moulding width" bind:value={style.moulding_width} {units} />
		<LengthInput label="Gap" bind:value={style.gap} {units} hint="Around every box" />
		<LengthInput
			label="Target box width"
			bind:value={style.target_box_width}
			{units}
			hint="Boxes are kept equal"
		/>
	{:else if style.kind === 'board_and_batten'}
		<LengthInput label="Height" bind:value={style.height} {units} hint="Floor to top of rail" />
		<LengthInput label="Top rail width" bind:value={style.top_rail_width} {units} />
		<LengthInput label="Batten width" bind:value={style.batten_width} {units} />
		<LengthInput
			label="Target spacing"
			bind:value={style.target_spacing}
			{units}
			hint="Centre to centre"
		/>
	{:else}
		<LengthInput label="Height" bind:value={style.height} {units} hint="Floor to top of rail" />
		<LengthInput label="Top rail width" bind:value={style.top_rail_width} {units} />
		<LengthInput label="Bottom rail width" bind:value={style.bottom_rail_width} {units} />
		<LengthInput label="Stile width" bind:value={style.stile_width} {units} />
		<LengthInput
			label="Target panel width"
			bind:value={style.target_panel_width}
			{units}
			hint="Between stiles"
		/>
	{/if}
	<LengthInput label="Baseboard height" bind:value={design.baseboard.height} {units} />
</div>

<label class="check">
	<input type="checkbox" bind:checked={design.baseboard.existing} />
	Keep the existing baseboard (leave it off the cut list)
</label>

<style>
	.styles {
		display: grid;
		gap: 0.5rem;
		margin-bottom: 1rem;
	}
	.styles button {
		display: grid;
		gap: 0.15rem;
		text-align: left;
		padding: 0.6rem 0.75rem;
	}
	.styles span {
		color: var(--ink-soft);
		font-size: 0.85rem;
	}
	.styles .selected {
		border-color: var(--accent);
		box-shadow: inset 0 0 0 1px var(--accent);
	}
</style>
