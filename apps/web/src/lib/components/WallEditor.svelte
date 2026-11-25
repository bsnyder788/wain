<script lang="ts">
	import {
		parseLength,
		type Length,
		type ObstructionKind,
		type UnitPrefs,
		type Wall
	} from '#lib/engine/index.ts';
	import { CORNER_NAMES, OBSTRUCTION_NAMES } from '#lib/labels.ts';
	import LengthInput from './LengthInput.svelte';

	type Props = {
		wall: Wall;
		units: UnitPrefs;
		/** Name of the wall this one's right end leads to, if there is one. */
		nextWall?: string;
	};
	let { wall = $bindable(), units, nextWall }: Props = $props();

	const CORNERS = Object.keys(CORNER_NAMES) as (keyof typeof CORNER_NAMES)[];

	const KINDS = Object.keys(OBSTRUCTION_NAMES) as ObstructionKind[];

	// Typical sizes [y above floor, width, height], measured to the outside
	// of the casing or cover plate, in each system's own round numbers.
	const TYPICAL: Record<ObstructionKind, { imperial: string[]; metric: string[] }> = {
		door: { imperial: ['0', '36', '82'], metric: ['0', '900', '2080'] },
		opening: { imperial: ['0', '48', '82'], metric: ['0', '1200', '2080'] },
		window: { imperial: ['36', '40', '52'], metric: ['900', '1000', '1300'] },
		outlet: { imperial: ['12', '2 3/4', '4 1/2'], metric: ['300', '80', '80'] },
		switch: { imperial: ['46', '2 3/4', '4 1/2'], metric: ['1100', '80', '80'] },
		vent: { imperial: ['6', '12', '6'], metric: ['150', '300', '150'] },
		other: { imperial: ['24', '12', '12'], metric: ['600', '300', '300'] }
	};

	function length(text: string): Length {
		const result = parseLength(text, units.system);
		return 'ok' in result ? result.ok : 0;
	}

	function add(kind: ObstructionKind) {
		const [y, w, h] = TYPICAL[kind][units.system].map(length);
		const width = Math.min(w, wall.width);
		const height = Math.min(h, wall.height - y);
		// Start it in the middle of the wall; the user then sets the real position.
		const x = Math.round((wall.width - width) / 2);
		wall.obstructions.push({ kind, rect: { x, y, width, height } });
	}
</script>

<div class="fields">
	<label class="field wide">
		<span class="field-label">Name</span>
		<input type="text" bind:value={wall.name} maxlength="60" />
	</label>
	<LengthInput label="Width" bind:value={wall.width} {units} hint="Corner to corner" />
	<LengthInput label="Ceiling height" bind:value={wall.height} {units} />
	{#if nextWall}
		<label class="field wide">
			<span class="field-label">Right-hand end</span>
			<select bind:value={wall.corner}>
				{#each CORNERS as corner (corner)}
					<option value={corner}>{CORNER_NAMES[corner](nextWall)}</option>
				{/each}
			</select>
			<span class="field-hint">Walls go around the room in order, left to right.</span>
		</label>
	{/if}
</div>

<h3>Doors, windows and outlets</h3>
<p class="note">Measure to the outside edge of the casing or cover plate.</p>

{#each wall.obstructions as obstruction, index (index)}
	<fieldset class="obstruction">
		<div class="obstruction-head">
			<select bind:value={obstruction.kind} aria-label="Type">
				{#each KINDS as kind (kind)}
					<option value={kind}>{OBSTRUCTION_NAMES[kind]}</option>
				{/each}
			</select>
			<button
				type="button"
				class="quiet"
				onclick={() => wall.obstructions.splice(index, 1)}
				aria-label="Remove {OBSTRUCTION_NAMES[obstruction.kind]}"
			>
				Remove
			</button>
		</div>
		<div class="fields">
			<LengthInput label="From left" bind:value={obstruction.rect.x} {units} />
			<LengthInput label="Above floor" bind:value={obstruction.rect.y} {units} />
			<LengthInput label="Width" bind:value={obstruction.rect.width} {units} />
			<LengthInput label="Height" bind:value={obstruction.rect.height} {units} />
		</div>
	</fieldset>
{/each}

<div class="add-row">
	{#each ['door', 'window', 'outlet', 'switch'] as const as kind (kind)}
		<button type="button" onclick={() => add(kind)}>+ {OBSTRUCTION_NAMES[kind]}</button>
	{/each}
</div>

<style>
	h3 {
		margin: 1.25rem 0 0.25rem;
	}
	.obstruction {
		margin: 0.75rem 0 0;
		padding: 0.75rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
	}
	.obstruction-head {
		display: flex;
		justify-content: space-between;
		gap: 0.5rem;
		margin-bottom: 0.5rem;
	}
	.add-row {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-top: 0.75rem;
	}
</style>
