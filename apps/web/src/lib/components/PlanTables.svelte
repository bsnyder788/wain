<script lang="ts">
	import { formatLength, type Length, type Plan, type Profile, type UnitPrefs } from '#lib/engine/index.ts';
	import { endCuts, MATERIAL_NAMES, PART_NAMES } from '#lib/labels.ts';

	type Props = {
		plan: Plan;
		units: UnitPrefs;
		/** Adds a box to tick beside each line, for working from a printout. */
		checklist?: boolean;
	};
	let { plan, units, checklist = false }: Props = $props();

	const fmt = (length: Length) => formatLength(length, units);
	// Lumber is sold by the foot, so stock lengths read better as 8' 0".
	const fmtStock = (length: Length) => formatLength(length, { ...units, feet_and_inches: true });
	const profileName = (profile: Profile) =>
		`${MATERIAL_NAMES[profile.material]}, ${fmt(profile.width)} wide`;
</script>

<section>
	<h2>Cut list</h2>
	{#if plan.cut_list.length === 0}
		<p class="note">Nothing to cut yet. The wall may be too small for this design.</p>
	{:else}
		<div class="scroll">
			<table>
				<thead>
					<tr>
						<th>Part</th>
						<th>Piece</th>
						<th>Cut from</th>
						<th class="num">Length</th>
						<th>Ends</th>
						<th class="num">Qty</th>
						{#if checklist}<th class="num">Cut</th>{/if}
					</tr>
				</thead>
				<tbody>
					{#each plan.cut_list as item (item.label)}
						<tr>
							<td><span class="tag">{item.label}</span></td>
							<td>{PART_NAMES[item.kind]}</td>
							<td>{profileName(item.profile)}</td>
							<td class="num">{fmt(item.length)}</td>
							<td>{endCuts(item.cuts)}</td>
							<td class="num">{item.quantity}</td>
							{#if checklist}<td class="num"><span class="tick"></span></td>{/if}
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
		<p class="note">
			Mitered lengths are to the long point; coped ends measure to the wall. Two different ends are
			listed left then right as the piece sits on the wall. Check every measurement against the
			wall before cutting.
		</p>
	{/if}
</section>

{#if plan.stock.length > 0}
	<section>
		<h2>Shopping list</h2>
		{#each plan.stock as group (group.profile.material + group.profile.width)}
			{@const longest = Math.max(...group.boards.map((board) => board.stock_length))}
			<div class="group">
				<h3>
					{profileName(group.profile)}
					<span class="buy">
						{group.purchase.map((p) => `${p.quantity} × ${fmtStock(p.stock_length)}`).join(', ')}
					</span>
				</h3>
				{#each group.boards as board, index (index)}
					<div
						class="board"
						style:width="{(board.stock_length / longest) * 100}%"
						title="{fmtStock(board.stock_length)} board"
					>
						{#each board.cuts as cut, cutIndex (cutIndex)}
							<span class="cut" style:flex-grow={cut.length}>
								<b>{cut.label}</b>
								{fmt(cut.length)}
							</span>
						{/each}
						{#if board.offcut > 0}
							<span class="offcut" style:flex-grow={board.offcut}></span>
						{/if}
					</div>
				{/each}
			</div>
		{/each}
		<p class="note">
			Each bar is one board drawn to scale, cut left to right; the hatched end is the offcut. Buy
			a little extra for defects and mistakes.
		</p>
	</section>
{/if}

<style>
	.scroll {
		overflow-x: auto;
	}
	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.95rem;
	}
	th,
	td {
		padding: 0.45rem 0.6rem;
		text-align: left;
		border-bottom: 1px solid var(--border);
		white-space: nowrap;
	}
	th {
		font-size: 0.8rem;
		color: var(--ink-soft);
		font-weight: 600;
	}
	.num {
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	.tick {
		display: inline-block;
		width: 1em;
		height: 1em;
		border: 1px solid var(--line);
		border-radius: 0.15em;
		vertical-align: middle;
	}
	.tag,
	.cut,
	.offcut {
		/* Keep the colours when printed; browsers drop backgrounds by default. */
		print-color-adjust: exact;
		-webkit-print-color-adjust: exact;
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
	}
	.group {
		margin-top: 1rem;
	}
	.group h3 {
		display: flex;
		flex-wrap: wrap;
		justify-content: space-between;
		gap: 0.25rem 1rem;
		margin: 0 0 0.4rem;
	}
	.buy {
		font-weight: 700;
		color: var(--accent);
	}
	.board {
		display: flex;
		height: 1.9rem;
		margin-bottom: 0.3rem;
		border: 1px solid var(--line-soft);
		border-radius: 0.25rem;
		overflow: hidden;
		font-size: 0.75rem;
	}
	.cut {
		flex-basis: 0;
		min-width: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 0.3em;
		background: var(--trim);
		border-right: 1px solid var(--line-soft);
		white-space: nowrap;
		overflow: hidden;
	}
	.cut b {
		color: var(--accent);
	}
	.offcut {
		flex-basis: 0;
		background: repeating-linear-gradient(
			-45deg,
			var(--surface),
			var(--surface) 4px,
			var(--border) 4px,
			var(--border) 5px
		);
	}
</style>
