<script lang="ts">
	import { account, setCredits } from '#lib/account.svelte.ts';
	import { ApiError, suggestDesigns, type Suggestion } from '#lib/api.ts';
	import { formatLength, type Design, type Project } from '#lib/engine/index.ts';
	import { ideas, saveIdeas } from '#lib/ideas.svelte.ts';
	import { STYLE_NAMES } from '#lib/labels.ts';
	import { loadPhoto } from '#lib/photos.ts';

	type Props = { project: Project; wallIndex: number };
	let { project = $bindable(), wallIndex }: Props = $props();

	const wall = $derived(project.walls[wallIndex]);
	const result = $derived(ideas[wall.id]);
	// Not named `state`: that would shadow the `$state` rune.
	const who = $derived(account.current);

	let busy = $state(false);
	let failure = $state<string | null>(null);

	async function suggest() {
		const photo = wall.photo;
		if (!photo) return;
		busy = true;
		failure = null;
		try {
			const image = await loadPhoto(photo.image_id);
			if (!image) throw new ApiError(0, 'missing', 'This photo is no longer stored on this device.');
			const response = await suggestDesigns(project, wallIndex, image);
			saveIdeas(wall.id, response);
			setCredits(response.credits);
		} catch (error) {
			failure =
				error instanceof ApiError && error.code === 'insufficient_credits'
					? 'You are out of credits. Buying more is coming soon.'
					: error instanceof ApiError
						? error.message
						: 'Something went wrong. Try again.';
		} finally {
			busy = false;
		}
	}

	// Plain copies, compared as text: the project's design is a reactive proxy.
	const current = $derived(JSON.stringify(project.design));
	const inUse = (design: Design) => JSON.stringify(design) === current;

	function use(suggestion: Suggestion) {
		// A plain copy, so later edits to the design do not alter the suggestion.
		project.design = $state.snapshot(suggestion.design);
	}

	function summary(design: Design): string {
		const style = design.style;
		const fmt = (length: number) => formatLength(length, project.units);
		switch (style.kind) {
			case 'picture_frame':
				return `Chair rail at ${fmt(style.chair_rail_height)}, boxes about ${fmt(style.target_box_width)} wide`;
			case 'board_and_batten':
				return `${fmt(style.height)} high, ${fmt(style.batten_width)} battens about ${fmt(style.target_spacing)} apart`;
			case 'shaker_panel':
				return `${fmt(style.height)} high, panels about ${fmt(style.target_panel_width)} wide`;
		}
	}
</script>

<section class="ideas">
	<div class="head">
		<h3>Design ideas</h3>
		{#if who.status === 'signed_in'}
			<span class="credits">{who.user.credits} credits left</span>
		{/if}
	</div>

	{#if who.status === 'signed_in'}
		<p class="lead">
			Let the AI look at this photo and suggest treatments that suit the room. The photo is sent to
			our AI provider, Anthropic, to be analysed.
		</p>
		<button type="button" class="primary" onclick={suggest} disabled={busy} aria-busy={busy}>
			{busy ? 'Looking at your wall…' : result ? 'Suggest again (1 credit)' : 'Suggest designs (1 credit)'}
		</button>
	{:else if who.status === 'signed_out'}
		<p class="lead">
			<a href="/account">Sign in</a> to have the AI look at this photo and suggest treatments that suit
			the room. New accounts start with 10 free credits.
		</p>
	{:else if who.status === 'offline'}
		<p class="lead">AI suggestions are unavailable because the server cannot be reached.</p>
	{/if}

	{#if failure}<p class="field-error" role="alert">{failure}</p>{/if}

	{#if result}
		{#if result.observations}<p class="observations">{result.observations}</p>{/if}
		<ul>
			{#each result.suggestions as suggestion, index (index)}
				{@const active = inUse(suggestion.design)}
				<li class:active>
					<div>
						<strong>{suggestion.title}</strong>
						<span class="style">{STYLE_NAMES[suggestion.design.style.kind]}</span>
					</div>
					<p>{suggestion.rationale}</p>
					<p class="dims">{summary(suggestion.design)}</p>
					<button type="button" onclick={() => use(suggestion)} disabled={active}>
						{active ? 'In use' : 'Use this design'}
					</button>
				</li>
			{/each}
		</ul>
		<p class="note">
			Suggestions are a starting point. Every dimension can still be changed under Design.
		</p>
	{/if}
</section>

<style>
	.ideas {
		margin-top: 1.25rem;
		padding-top: 1rem;
		border-top: 1px solid var(--border);
	}
	.head {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: 1rem;
	}
	.credits {
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
	.lead {
		margin: 0 0 0.75rem;
		max-width: 40rem;
		color: var(--ink-soft);
		font-size: 0.9rem;
	}
	.primary {
		border-color: var(--accent);
		background: var(--accent);
		color: white;
		font-weight: 600;
	}
	.primary:disabled {
		opacity: 0.7;
		cursor: progress;
	}
	.observations {
		margin: 1rem 0 0;
		font-style: italic;
	}
	ul {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(15rem, 1fr));
		gap: 0.75rem;
		margin: 0.75rem 0 0;
		padding: 0;
		list-style: none;
	}
	li {
		display: grid;
		gap: 0.4rem;
		align-content: start;
		padding: 0.75rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
	}
	li.active {
		border-color: var(--accent);
		box-shadow: inset 0 0 0 1px var(--accent);
	}
	li p {
		margin: 0;
		font-size: 0.9rem;
	}
	.style {
		display: block;
		font-size: 0.8rem;
		color: var(--ink-soft);
	}
	.dims {
		color: var(--ink-soft);
	}
	li button {
		justify-self: start;
		margin-top: 0.25rem;
	}
	li button:disabled {
		color: var(--accent);
		font-weight: 600;
		cursor: default;
	}
</style>
