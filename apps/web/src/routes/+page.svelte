<script lang="ts">
	import DesignEditor from '#lib/components/DesignEditor.svelte';
	import Elevation from '#lib/components/Elevation.svelte';
	import PlanTables from '#lib/components/PlanTables.svelte';
	import WallEditor from '#lib/components/WallEditor.svelte';
	import { planProject, type Plan } from '#lib/engine/index.ts';
	import { addWall, removeWall, resetProject, session } from '#lib/session.svelte.ts';

	const project = $derived(session.project);
	const units = $derived(project.units);

	let selected = $state(0);
	const wallIndex = $derived(Math.min(selected, project.walls.length - 1));

	const result = $derived(planProject(project));
	const issues = $derived('error' in result ? result.error.issues : []);

	// While the project is invalid (say, mid-edit) keep showing the last plan
	// that worked instead of blanking the drawing.
	let lastPlan: Plan | undefined;
	const plan = $derived.by(() => {
		if ('ok' in result) lastPlan = result.ok;
		return lastPlan;
	});
	const stale = $derived(issues.length > 0);
</script>

<div class="editor">
	<aside>
		<section class="panel">
			<div class="row">
				<h2>Walls</h2>
				<button type="button" class="quiet" onclick={resetProject}>Start over</button>
			</div>
			<div class="tabs" role="tablist">
				{#each project.walls as wall, index (wall.id)}
					<button
						type="button"
						role="tab"
						aria-selected={index === wallIndex}
						onclick={() => (selected = index)}
					>
						{wall.name || `Wall ${index + 1}`}
					</button>
				{/each}
				<button type="button" onclick={() => (selected = addWall())} aria-label="Add a wall">
					+
				</button>
			</div>
			<WallEditor bind:wall={project.walls[wallIndex]} {units} />
			{#if project.walls.length > 1}
				<button type="button" class="quiet remove" onclick={() => removeWall(wallIndex)}>
					Remove this wall
				</button>
			{/if}
		</section>

		<section class="panel">
			<h2>Design</h2>
			<DesignEditor bind:design={project.design} {units} />
		</section>
	</aside>

	<div class="output">
		{#if issues.length > 0}
			<div class="banner error" role="alert">
				<strong>Fix these to update the plan:</strong>
				<ul>
					{#each issues as issue (issue.path)}
						<li><code>{issue.path}</code> {issue.message}</li>
					{/each}
				</ul>
			</div>
		{/if}

		{#if plan}
			{#each plan.warnings as warning, index (index)}
				<div class="banner" role="status">{warning.message}</div>
			{/each}

			<section class="panel" class:stale>
				<Elevation
					wall={project.walls[wallIndex]}
					layout={plan.walls[wallIndex]}
					design={project.design}
					{units}
				/>
			</section>

			<div class="panel tables" class:stale>
				<PlanTables {plan} {units} />
			</div>
		{/if}
	</div>
</div>

<style>
	.editor {
		display: grid;
		grid-template-columns: minmax(18rem, 24rem) minmax(0, 1fr);
		gap: 1rem;
		align-items: start;
	}
	aside,
	.output,
	.tables {
		display: grid;
		/* Let wide content (the cut list) scroll inside its panel instead of
		   stretching the page. */
		grid-template-columns: minmax(0, 1fr);
		gap: 1rem;
	}
	.row {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
	}
	.tabs {
		display: flex;
		flex-wrap: wrap;
		gap: 0.35rem;
		margin-bottom: 0.9rem;
	}
	.tabs [aria-selected='true'] {
		border-color: var(--accent);
		color: var(--accent);
		font-weight: 600;
	}
	.remove {
		margin-top: 0.75rem;
	}
	.banner {
		padding: 0.7rem 0.9rem;
		border-radius: var(--radius);
		background: var(--warn-bg);
		font-size: 0.9rem;
	}
	.banner.error {
		background: #fbe9e5;
	}
	.banner ul {
		margin: 0.3rem 0 0;
		padding-left: 1.1rem;
	}
	.stale {
		opacity: 0.45;
	}
	.tables {
		gap: 1.5rem;
	}
	@media (max-width: 52rem) {
		.editor {
			grid-template-columns: minmax(0, 1fr);
		}
		/* On a phone the drawing matters most, so it goes first. */
		.output {
			order: -1;
		}
	}
</style>
