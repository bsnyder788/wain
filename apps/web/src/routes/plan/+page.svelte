<script lang="ts">
	import Elevation from '#lib/components/Elevation.svelte';
	import PlanTables from '#lib/components/PlanTables.svelte';
	import { formatLength, planProject, type Length } from '#lib/engine/index.ts';
	import { jointNote, OBSTRUCTION_NAMES } from '#lib/labels.ts';
	import { designFacts, placements } from '#lib/plan-sheet.ts';
	import { session } from '#lib/session.svelte.ts';

	const project = $derived(session.project);
	const units = $derived(project.units);
	const result = $derived(planProject(project));
	const plan = $derived('ok' in result ? result.ok : undefined);
	const issues = $derived('error' in result ? result.error.issues : []);

	const fmt = (length: Length) => formatLength(length, units);
	const today = new Date().toLocaleDateString(undefined, { dateStyle: 'long' });

	const count = $derived(project.walls.length);
	const nameOf = (index: number) => project.walls[(index + count) % count].name || 'the next wall';
	const boards = $derived(project.design.style.kind !== 'picture_frame');

	function cornerNotes(index: number) {
		const layout = plan?.walls[index];
		if (!layout) return [];
		return [
			{ end: 'Left end', note: jointNote(layout.left_joint, nameOf(index - 1), boards) },
			{ end: 'Right end', note: jointNote(layout.right_joint, nameOf(index + 1), boards) }
		].filter((entry) => entry.note);
	}
</script>

<svelte:head>
	<title>{project.name || 'Plan'} · Wain</title>
</svelte:head>

<div class="sheet">
	<div class="toolbar no-print">
		<label class="field">
			<span class="field-label">Project name</span>
			<input type="text" bind:value={project.name} maxlength="80" />
		</label>
		<button type="button" class="primary" onclick={() => window.print()} disabled={!plan}>
			Print or save as PDF
		</button>
	</div>

	{#if issues.length > 0}
		<div class="banner" role="alert">
			<strong>This design has problems, so there is no plan yet.</strong>
			<a href="/">Go back to the design</a> to fix them.
		</div>
	{/if}

	{#if plan}
		<section class="page">
			<h1>{project.name || 'Wall trim plan'}</h1>
			<p class="meta">Prepared {today} · {count} {count === 1 ? 'wall' : 'walls'}</p>

			<dl class="facts">
				{#each designFacts(project.design, units) as [label, value] (label)}
					<div>
						<dt>{label}</dt>
						<dd>{value}</dd>
					</div>
				{/each}
			</dl>

			{#if plan.warnings.length > 0}
				<div class="warnings">
					<h2>Before you start</h2>
					<ul>
						{#each plan.warnings as warning, index (index)}
							<li>{warning.message}</li>
						{/each}
					</ul>
				</div>
			{/if}

			<h2>How to use this plan</h2>
			<ol class="steps">
				<li>
					Check each wall's width and ceiling height against the drawing. If your wall differs,
					change it in the design and print again: every length here depends on it.
				</li>
				<li>Buy the stock on the shopping list and cut the pieces on the cut list.</li>
				<li>
					Mark the heights from each wall's drawing with a level line. Floors are rarely level, so
					measure up from the highest point of the floor.
				</li>
				<li>
					Fix the pieces where the placement table says, working through the walls in order.
					Measurements run from the wall's left end and up from the floor.
				</li>
			</ol>
		</section>

		{#each project.walls as wall, index (wall.id)}
			{@const layout = plan.walls[index]}
			<section class="page wall">
				<h2>{wall.name || `Wall ${index + 1}`}</h2>
				<p class="meta">{fmt(wall.width)} wide · {fmt(wall.height)} to the ceiling</p>

				<div class="drawing">
					<Elevation {wall} {layout} design={project.design} {units} levels />
				</div>
				{#each cornerNotes(index) as entry (entry.end)}
					<p class="note"><strong>{entry.end}:</strong> {entry.note}</p>
				{/each}

				{#if wall.obstructions.length > 0}
					<h3>On this wall</h3>
					<table>
						<thead>
							<tr>
								<th>Item</th>
								<th class="num">From left</th>
								<th class="num">Above floor</th>
								<th class="num">Width</th>
								<th class="num">Height</th>
							</tr>
						</thead>
						<tbody>
							{#each wall.obstructions as obstruction, row (row)}
								<tr>
									<td>{OBSTRUCTION_NAMES[obstruction.kind]}</td>
									<td class="num">{fmt(obstruction.rect.x)}</td>
									<td class="num">{fmt(obstruction.rect.y)}</td>
									<td class="num">{fmt(obstruction.rect.width)}</td>
									<td class="num">{fmt(obstruction.rect.height)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				{/if}

				<h3>Where each piece goes</h3>
				{#if layout.parts.length === 0}
					<p class="note">No trim fits on this wall with the current design.</p>
				{:else}
					<table>
						<thead>
							<tr>
								<th>Part</th>
								<th>Piece</th>
								<th class="num">Left edge from left end</th>
								<th class="num">Bottom above floor</th>
								<th class="num">Width</th>
								<th class="num">Height</th>
							</tr>
						</thead>
						<tbody>
							{#each placements(layout) as row, line (line)}
								<tr>
									<td><span class="tag">{row.label}</span></td>
									<td>{row.piece}</td>
									<td class="num">{fmt(row.fromLeft)}</td>
									<td class="num">{fmt(row.aboveFloor)}</td>
									<td class="num">{fmt(row.width)}</td>
									<td class="num">{fmt(row.height)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				{/if}
			</section>
		{/each}

		<section class="page lists">
			<PlanTables {plan} {units} checklist />
		</section>

		<footer>
			Made with Wain. Dimensions come from the measurements entered; check each one against the
			wall before cutting.
		</footer>
	{/if}
</div>

<style>
	.sheet {
		max-width: 56rem;
		margin: 0 auto;
		display: grid;
		gap: 1.5rem;
	}
	.toolbar {
		display: flex;
		flex-wrap: wrap;
		align-items: end;
		justify-content: space-between;
		gap: 0.75rem;
	}
	.toolbar .field {
		flex: 1 1 16rem;
	}
	.primary {
		border-color: var(--accent);
		background: var(--accent);
		color: white;
		font-weight: 600;
	}
	.banner {
		padding: 0.7rem 0.9rem;
		border-radius: var(--radius);
		background: #fbe9e5;
	}
	.page {
		padding: 1.25rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--panel);
	}
	h1 {
		font-size: 1.6rem;
	}
	h2 {
		font-size: 1.2rem;
		margin-top: 1.25rem;
	}
	.page > h2:first-child {
		margin-top: 0;
	}
	h3 {
		margin: 1.25rem 0 0.4rem;
	}
	.meta {
		margin: 0;
		color: var(--ink-soft);
	}
	.facts {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(11rem, 1fr));
		gap: 0.6rem 1rem;
		margin: 1rem 0 0;
	}
	.facts dt {
		font-size: 0.8rem;
		font-weight: 600;
		color: var(--ink-soft);
	}
	.facts dd {
		margin: 0;
	}
	.warnings {
		margin-top: 1rem;
		padding: 0.1rem 0.9rem 0.6rem;
		border-radius: var(--radius);
		background: var(--warn-bg);
		print-color-adjust: exact;
		-webkit-print-color-adjust: exact;
	}
	.warnings ul,
	.steps {
		margin: 0.4rem 0 0;
		padding-left: 1.2rem;
	}
	.steps li {
		margin-bottom: 0.35rem;
	}
	.drawing {
		margin-top: 0.75rem;
	}
	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.92rem;
	}
	th,
	td {
		padding: 0.35rem 0.5rem;
		text-align: left;
		border-bottom: 1px solid var(--border);
	}
	th {
		font-size: 0.78rem;
		color: var(--ink-soft);
		font-weight: 600;
		vertical-align: bottom;
	}
	.num {
		text-align: right;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
	th.num {
		white-space: normal;
	}
	.tag {
		display: inline-block;
		min-width: 1.6em;
		padding: 0.05em 0.35em;
		border-radius: 0.3em;
		background: var(--accent);
		color: white;
		font-weight: 700;
		text-align: center;
		white-space: nowrap;
		print-color-adjust: exact;
		-webkit-print-color-adjust: exact;
	}
	footer {
		font-size: 0.8rem;
		color: var(--ink-soft);
		text-align: center;
	}

	@media print {
		.sheet {
			display: block;
			max-width: none;
		}
		.page {
			border: none;
			padding: 0;
		}
		/* Each wall, and the lists, start on a fresh sheet. */
		.wall,
		.lists {
			break-before: page;
		}
		.drawing,
		tr {
			break-inside: avoid;
		}
		/* A narrow wall is nearly square; without a cap its drawing would
		   fill the sheet and push the table onto another. */
		.drawing :global(svg) {
			max-height: 105mm;
		}
		thead {
			display: table-header-group;
		}
		footer {
			margin-top: 1rem;
		}
	}
</style>
