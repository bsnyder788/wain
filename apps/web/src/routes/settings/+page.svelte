<script lang="ts">
	import { formatLength, type UnitPrefs, type UnitSystem } from '#lib/engine/index.ts';
	import { session, setUnits } from '#lib/session.svelte.ts';

	const units = $derived(session.project.units);
	const update = (change: Partial<UnitPrefs>) => setUnits({ ...units, ...change });

	const SYSTEMS: { value: UnitSystem; label: string; example: string }[] = [
		{ value: 'imperial', label: 'Imperial', example: 'Inches and fractions, lumber sold by the foot' },
		{ value: 'metric', label: 'Metric', example: 'Millimetres, lumber sold by the metre' }
	];
	const PRECISIONS = [8, 16, 32];

	// 32 5/32" shows how each setting rounds and whether feet are split out.
	const SAMPLE = (32 + 5 / 32) * 25_400_000;
</script>

<div class="settings">
	<h1>Settings</h1>

	<section class="panel">
		<h2>Units</h2>
		<div class="options" role="radiogroup" aria-label="Unit system">
			{#each SYSTEMS as system (system.value)}
				<label class="option">
					<input
						type="radio"
						name="system"
						checked={units.system === system.value}
						onchange={() => update({ system: system.value })}
					/>
					<span>
						<strong>{system.label}</strong>
						<small>{system.example}</small>
					</span>
				</label>
			{/each}
		</div>

		{#if units.system === 'imperial'}
			<div class="fields">
				<label class="field">
					<span class="field-label">Smallest fraction</span>
					<select
						value={units.fraction_denominator}
						onchange={(event) => update({ fraction_denominator: Number(event.currentTarget.value) })}
					>
						{#each PRECISIONS as precision (precision)}
							<option value={precision}>1/{precision}"</option>
						{/each}
					</select>
				</label>
			</div>
			<label class="check">
				<input
					type="checkbox"
					checked={units.feet_and_inches}
					onchange={(event) => update({ feet_and_inches: event.currentTarget.checked })}
				/>
				Show feet and inches
			</label>
		{/if}

		<p class="sample">Lengths will look like <strong>{formatLength(SAMPLE, units)}</strong></p>
		<p class="note">
			You can type a length in any unit whatever this is set to, such as <code>32 1/2"</code>,
			<code>2' 8"</code> or <code>826mm</code>. Changing the unit system keeps your design the same
			size and switches the stock lengths on the shopping list.
		</p>
	</section>
</div>

<style>
	.settings {
		max-width: 36rem;
		display: grid;
		gap: 1rem;
	}
	.options {
		display: grid;
		gap: 0.5rem;
		margin-bottom: 1rem;
	}
	.option {
		display: flex;
		gap: 0.6rem;
		padding: 0.6rem 0.75rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		cursor: pointer;
	}
	.option:has(input:checked) {
		border-color: var(--accent);
		box-shadow: inset 0 0 0 1px var(--accent);
	}
	.option span {
		display: grid;
	}
	.option small {
		color: var(--ink-soft);
	}
	.sample {
		margin: 1rem 0 0;
	}
</style>
