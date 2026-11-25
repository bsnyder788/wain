<script lang="ts">
	import { detectObstructions, detectOutline, photoGrid } from '#lib/detect.ts';
	import {
		formatLength,
		wallHomography,
		type Design,
		type Length,
		type Obstruction,
		type ObstructionKind,
		type Point,
		type Rect,
		type SidesFound,
		type UnitPrefs,
		type Wall,
		type WallLayout,
		type WallPhoto
	} from '#lib/engine/index.ts';
	import { OBSTRUCTION_NAMES } from '#lib/labels.ts';
	import { rectPoints, toPhoto, toWall } from '#lib/perspective.ts';
	import { loadPhoto } from '#lib/photos.ts';
	import { removePhoto, setPhoto } from '#lib/session.svelte.ts';

	type Props = {
		wall: Wall;
		layout: WallLayout | undefined;
		design: Design;
		units: UnitPrefs;
	};
	let { wall = $bindable(), layout, design, units }: Props = $props();

	const MARKABLE = ['door', 'window', 'outlet', 'switch'] as const;
	const CORNER_NAMES = ['Top left', 'Top right', 'Bottom right', 'Bottom left'];

	/**
	 * `view` just shows the preview and `review` shows what was found on the
	 * wall for the user to accept; otherwise the user is placing something.
	 */
	type Mode = 'view' | 'corners' | 'review' | ObstructionKind;
	let mode = $state<Mode>('view');
	let showDesign = $state(true);
	let busy = $state(false);
	let failure = $state<string | null>(null);

	const photo = $derived(wall.photo);
	const mapping = $derived(wallHomography(wall));
	const homography = $derived(mapping && 'ok' in mapping ? mapping.ok : undefined);
	const marking = $derived(mode !== 'view' && mode !== 'corners' && mode !== 'review');

	// The stored image, loaded from this device whenever the wall's photo changes.
	let url = $state<string>();
	let missing = $state(false);
	$effect(() => {
		const id = photo?.image_id;
		url = undefined;
		missing = false;
		if (!id) return;

		let cancelled = false;
		let created: string | undefined;
		loadPhoto(id)
			.then((blob) => {
				if (cancelled) return;
				if (!blob) {
					missing = true;
					return;
				}
				created = URL.createObjectURL(blob);
				url = created;
			})
			.catch(() => {
				if (!cancelled) missing = true;
			});
		return () => {
			cancelled = true;
			if (created) URL.revokeObjectURL(created);
		};
	});

	async function choose(event: Event & { currentTarget: HTMLInputElement }) {
		const file = event.currentTarget.files?.[0];
		event.currentTarget.value = '';
		if (!file) return;
		busy = true;
		failure = null;
		try {
			await setPhoto(wall, file);
		} catch {
			failure = 'That file could not be read as a photo. Try a JPEG or PNG.';
			return;
		} finally {
			busy = false;
		}
		mode = 'corners';
		suggestions = [];
		notice = null;
		// A new photo gets the full treatment: its corners are found now, and
		// what is on the wall once the user is happy with the corners.
		unscanned = true;
		await findCorners();
	}

	function remove() {
		removePhoto(wall);
		mode = 'view';
		suggestions = [];
		notice = null;
		guess = null;
	}

	// ---- Finding things in the photo -----------------------------------------

	/** True while the photo is being analysed. */
	let looking = $state(false);
	/** How the last search for the wall's outline went, until the user moves on. */
	let guess = $state<{ sides: SidesFound; before: WallPhoto['corners'] } | 'none' | null>(null);
	/** Set for a photo added just now, whose wall has not been searched yet. */
	let unscanned = false;
	/** Things found on the wall, waiting for the user to accept or reject. */
	let suggestions = $state<{ obstruction: Obstruction; chosen: boolean }[]>([]);
	/** The outcome of the last action, shown until the next one. */
	let notice = $state<string | null>(null);

	const chosen = $derived(suggestions.filter((suggestion) => suggestion.chosen));

	/** Places the corners on the wall's outline, if one can be found. */
	async function findCorners() {
		const id = wall.photo?.image_id;
		if (!id || looking) return;
		looking = true;
		try {
			const found = await detectOutline(wall);
			// The photo may have been replaced or removed while this ran.
			const current = wall.photo;
			if (current?.image_id !== id) return;
			if (!found) {
				guess = 'none';
				return;
			}
			guess = { sides: found.sides, before: $state.snapshot(current.corners) };
			current.corners = found.corners.map(
				(corner): Point => ({ x: clampFraction(corner.x), y: clampFraction(corner.y) })
			) as WallPhoto['corners'];
		} catch {
			guess = 'none';
		} finally {
			looking = false;
		}
	}

	function undoCorners() {
		if (photo && guess && guess !== 'none') photo.corners = guess.before;
		guess = null;
	}

	function toggleCorners() {
		guess = null;
		notice = null;
		if (mode !== 'corners') {
			mode = 'corners';
			return;
		}
		mode = 'view';
		if (unscanned && homography) void findOnWall();
	}

	/** Looks for doors, windows, outlets and switches and offers what it finds. */
	async function findOnWall() {
		if (!homography || looking) return;
		unscanned = false;
		mode = 'view';
		notice = null;
		looking = true;
		try {
			const found = await detectObstructions(wall, grid);
			// Only if the user has not started on something else meanwhile.
			if (mode !== 'view') return;
			if (found.length > 0) {
				suggestions = found.map((obstruction) => ({ obstruction, chosen: true }));
				mode = 'review';
			} else {
				const already = wall.obstructions.length > 0 ? ' that are not already on the wall' : '';
				notice = `Found no doors, windows, outlets or switches${already}. Mark any there are with the buttons below.`;
			}
		} catch {
			notice = 'This photo could not be analysed. Mark things yourself with the buttons below.';
		} finally {
			looking = false;
		}
	}

	function acceptSuggestions() {
		const added = chosen.map((suggestion) => $state.snapshot(suggestion.obstruction));
		wall.obstructions.push(...added);
		const things = added.length === 1 ? 'It is' : 'They are';
		notice =
			added.length > 0
				? `Added ${added.length} to the wall. ${things} in the wall's list, where you can correct the sizes.`
				: null;
		suggestions = [];
		mode = 'view';
	}

	function dismissSuggestions() {
		suggestions = [];
		mode = 'view';
	}

	function describe(obstruction: Obstruction): string {
		const { kind, rect } = obstruction;
		const size = `${fmt(rect.width)} × ${fmt(rect.height)}`;
		// A door stands on the floor, so only its position along the wall matters.
		const where =
			rect.y === 0
				? `${fmt(rect.x)} from the left`
				: `${fmt(rect.x)} from the left, ${fmt(rect.y)} up`;
		return `${OBSTRUCTION_NAMES[kind]}, ${size}, ${where}`;
	}

	const SIDE_NAMES: [keyof SidesFound, string][] = [
		['top', 'ceiling line'],
		['bottom', 'floor line'],
		['left', 'left-hand end'],
		['right', 'right-hand end']
	];

	function list(items: string[]): string {
		return items.length <= 1
			? items.join('')
			: `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;
	}

	// ---- Drawing, in photo pixels -------------------------------------------

	const outline = $derived(
		photo?.corners.map((p) => `${p.x * photo.width},${p.y * photo.height}`).join(' ') ?? ''
	);
	const handle = $derived(photo ? Math.max(photo.width, photo.height) * 0.011 : 0);

	const treatmentTop = $derived(
		design.style.kind === 'picture_frame' ? design.style.chair_rail_height : design.style.height
	);

	/**
	 * The panelled area shown as painted: from the baseboard up to the top of
	 * the treatment, with doors and windows cut out of it.
	 */
	const paint = $derived.by(() => {
		if (!homography) return '';
		const bottom = design.baseboard.height;
		const band: Rect = { x: 0, y: bottom, width: wall.width, height: treatmentTop - bottom };
		const holes = wall.obstructions
			.filter((o) => o.kind === 'door' || o.kind === 'opening' || o.kind === 'window')
			.map((o) => clip(o.rect, band))
			.filter((rect) => rect !== undefined);
		return [band, ...holes].map((rect) => `M${rectPoints(homography, rect)}Z`).join(' ');
	});

	function clip(rect: Rect, to: Rect): Rect | undefined {
		const x = Math.max(rect.x, to.x);
		const y = Math.max(rect.y, to.y);
		const right = Math.min(rect.x + rect.width, to.x + to.width);
		const top = Math.min(rect.y + rect.height, to.y + to.height);
		return right > x && top > y ? { x, y, width: right - x, height: top - y } : undefined;
	}

	// ---- Pointer handling ----------------------------------------------------

	let svg = $state<SVGSVGElement>();

	/** The pointer's position in photo pixels. */
	function locate(event: PointerEvent): [number, number] {
		const matrix = svg?.getScreenCTM();
		if (!matrix) return [0, 0];
		const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse());
		return [point.x, point.y];
	}

	// A corner can sit a little outside the frame when it is out of shot.
	const clampFraction = (value: number) => Math.min(1.25, Math.max(-0.25, value));

	function moveCorner(index: number, u: number, v: number) {
		if (!photo) return;
		photo.corners[index] = {
			x: clampFraction(u / photo.width),
			y: clampFraction(v / photo.height)
		};
	}

	let dragging = $state<number | null>(null);

	function startCorner(event: PointerEvent & { currentTarget: SVGElement }, index: number) {
		// From here the corners are the user's, so there is nothing to undo.
		guess = null;
		dragging = index;
		event.currentTarget.setPointerCapture(event.pointerId);
		event.preventDefault();
	}

	function dragCorner(event: PointerEvent) {
		if (dragging !== null) moveCorner(dragging, ...locate(event));
	}

	/** Arrow keys nudge a corner by a pixel, or ten with Shift. */
	function nudgeCorner(event: KeyboardEvent, index: number) {
		const step = event.shiftKey ? 10 : 1;
		const moves: Record<string, [number, number]> = {
			ArrowLeft: [-step, 0],
			ArrowRight: [step, 0],
			ArrowUp: [0, -step],
			ArrowDown: [0, step]
		};
		const move = moves[event.key];
		if (!move || !photo) return;
		event.preventDefault();
		guess = null;
		const corner = photo.corners[index];
		moveCorner(index, corner.x * photo.width + move[0], corner.y * photo.height + move[1]);
	}

	// Marking: the user drags between two opposite corners of a door, window
	// or outlet. Both ends are turned into wall positions, so the box follows
	// the wall's perspective rather than the screen's.
	let draft = $state<{ from: [Length, Length]; to: [Length, Length] } | null>(null);

	// Photo positions are estimates, so they snap to a coarse grid.
	const grid = $derived(photoGrid(units));
	const SMALLEST = 25_000_000;

	function draftRect(kind: ObstructionKind): Rect | undefined {
		if (!draft) return undefined;
		const snap = (value: number, limit: number) =>
			Math.min(limit, Math.max(0, Math.round(value / grid) * grid));
		const x = snap(Math.min(draft.from[0], draft.to[0]), wall.width);
		const right = snap(Math.max(draft.from[0], draft.to[0]), wall.width);
		const top = snap(Math.max(draft.from[1], draft.to[1]), wall.height);
		// A door or opening always reaches the floor.
		const floored = kind === 'door' || kind === 'opening';
		const y = floored ? 0 : snap(Math.min(draft.from[1], draft.to[1]), wall.height);
		return { x, y, width: right - x, height: top - y };
	}

	const draftShape = $derived(marking ? draftRect(mode as ObstructionKind) : undefined);

	function startMark(event: PointerEvent & { currentTarget: SVGSVGElement }) {
		if (!marking || !homography) return;
		const position = toWall(homography, ...locate(event));
		if (!position) return;
		draft = { from: position, to: position };
		event.currentTarget.setPointerCapture(event.pointerId);
		event.preventDefault();
	}

	function dragMark(event: PointerEvent) {
		if (!draft || !homography) return;
		const position = toWall(homography, ...locate(event));
		if (position) draft.to = position;
	}

	function endMark() {
		if (!draft || !marking) return;
		const kind = mode as ObstructionKind;
		const rect = draftRect(kind);
		draft = null;
		// A stray tap makes nothing; the tool stays armed for another try.
		if (!rect || rect.width < SMALLEST || rect.height < SMALLEST) return;
		wall.obstructions.push({ kind, rect });
		mode = 'view';
	}

	const fmt = (length: Length) => formatLength(length, units);
	const CORNERS_HELP =
		'Drag each dot onto a corner of the wall, where it meets the ceiling, the floor and the side walls. Arrow keys nudge the selected dot.';
	const hint = $derived.by(() => {
		if (looking) return 'Looking at the photo…';
		if (mapping && 'error' in mapping) {
			return 'The corners are crossed over. Drag them so they go around the wall in order.';
		}
		if (mode === 'corners') {
			if (guess === 'none') {
				return `Could not pick out the wall in this photo. ${CORNERS_HELP}`;
			}
			if (guess) {
				const sides = guess.sides;
				const missing = SIDE_NAMES.filter(([side]) => !sides[side]).map(([, name]) => name);
				if (missing.length === 0) {
					return 'Found the wall and put a dot on each corner. Check them, and drag any that are off.';
				}
				const found = SIDE_NAMES.filter(([side]) => sides[side]).map(([, name]) => name);
				return `Found the wall's ${list(found)}, but not its ${list(missing)}. The dots on the edge of the photo are guesses: drag them to where those corners are, even if that is just out of shot.`;
			}
			return CORNERS_HELP;
		}
		if (marking) {
			return `Drag from one corner of the ${OBSTRUCTION_NAMES[mode as ObstructionKind].toLowerCase()} to the opposite corner, including its casing or cover plate.`;
		}
		return notice;
	});
</script>

{#if !photo}
	<div class="empty">
		<h3>Preview on a photo</h3>
		<p>
			Add a photo of {wall.name || 'this wall'} to see the design on it. Stand back so the whole
			wall is in view, floor to ceiling if you can. The photo stays on this device.
		</p>
		<label class="button primary" aria-busy={busy}>
			{busy ? 'Adding…' : 'Add a photo'}
			<input type="file" accept="image/*" onchange={choose} disabled={busy} />
		</label>
		{#if failure}<p class="field-error" role="alert">{failure}</p>{/if}
	</div>
{:else}
	<!-- Sized by width so the overlay always matches the image exactly; the
	     width cap keeps a tall photo within the screen. -->
	<div
		class="frame"
		style:aspect-ratio="{photo.width} / {photo.height}"
		style:max-width="calc(75vh * {photo.width / photo.height})"
	>
		{#if url}
			<img src={url} alt="Photo of {wall.name}" draggable="false" />
		{:else if missing}
			<p class="lost">This photo is no longer stored on this device. Replace it to continue.</p>
		{/if}

		<svg
			bind:this={svg}
			viewBox="0 0 {photo.width} {photo.height}"
			class:marking
			role="img"
			aria-label="The design drawn over the photo of {wall.name}"
			onpointerdown={startMark}
			onpointermove={dragMark}
			onpointerup={endMark}
			onpointercancel={() => (draft = null)}
		>
			<!-- Hidden while reviewing, so what was found can be checked against
			     the photo itself. -->
			{#if homography && showDesign && mode !== 'review'}
				<path class="paint" d={paint} fill-rule="evenodd" />
				{#each layout?.parts ?? [] as part, index (index)}
					<polygon class="part" points={rectPoints(homography, part.rect)} />
				{/each}
			{/if}

			{#if homography && mode !== 'view'}
				{#each wall.obstructions as obstruction, index (index)}
					<polygon class="marked" points={rectPoints(homography, obstruction.rect)} />
				{/each}
			{/if}

			{#if homography && mode === 'review'}
				{#each suggestions as suggestion, index (index)}
					{@const rect = suggestion.obstruction.rect}
					{@const [u, v] = toPhoto(homography, rect.x + rect.width / 2, rect.y + rect.height / 2)}
					<!-- A shortcut for the tick boxes below the photo, which are
					     the real controls. -->
					<polygon
						class="suggested"
						class:off={!suggestion.chosen}
						points={rectPoints(homography, rect)}
						aria-hidden="true"
						onclick={() => (suggestion.chosen = !suggestion.chosen)}
					/>
					<text class="size" class:off={!suggestion.chosen} x={u} y={v} font-size={handle * 1.8}>
						{OBSTRUCTION_NAMES[suggestion.obstruction.kind]}
					</text>
				{/each}
			{/if}

			{#if homography && draftShape && draftShape.width > 0 && draftShape.height > 0}
				{@const [u, v] = toPhoto(
					homography,
					draftShape.x + draftShape.width / 2,
					draftShape.y + draftShape.height / 2
				)}
				<polygon class="draft" points={rectPoints(homography, draftShape)} />
				<text class="size" x={u} y={v} font-size={handle * 2.2}>
					{fmt(draftShape.width)} × {fmt(draftShape.height)}
				</text>
			{/if}

			{#if mode === 'corners'}
				<polygon class="outline" class:invalid={!homography} points={outline} />
				{#each photo.corners as corner, index (index)}
					<g
						class="handle"
						class:active={dragging === index}
						role="slider"
						tabindex="0"
						aria-label="{CORNER_NAMES[index]} corner of the wall"
						aria-valuetext="{Math.round(corner.x * 100)}% across, {Math.round(
							corner.y * 100
						)}% down"
						aria-valuenow={Math.round(corner.x * 100)}
						onpointerdown={(event) => startCorner(event, index)}
						onpointermove={dragCorner}
						onpointerup={() => (dragging = null)}
						onpointercancel={() => (dragging = null)}
						onkeydown={(event) => nudgeCorner(event, index)}
					>
						<!-- A generous invisible target, so a fingertip can grab it. -->
						<circle class="grab" cx={corner.x * photo.width} cy={corner.y * photo.height} r={handle * 4} />
						<circle class="dot" cx={corner.x * photo.width} cy={corner.y * photo.height} r={handle} />
					</g>
				{/each}
			{/if}
		</svg>
	</div>

	{#if hint}
		<p class="hint" role="status">
			{hint}
			{#if mode === 'corners' && guess && guess !== 'none' && !looking}
				<button type="button" class="inline" onclick={undoCorners}>Put them back</button>
			{/if}
		</p>
	{/if}

	{#if mode === 'review'}
		<fieldset class="found">
			<legend>Found on this wall</legend>
			<p>
				Untick anything that is wrong or that is not there, then add the rest. Sizes are read off
				the photo, so treat them as a starting point.
			</p>
			<ul>
				{#each suggestions as suggestion, index (index)}
					<li>
						<label>
							<input type="checkbox" bind:checked={suggestion.chosen} />
							{describe(suggestion.obstruction)}
						</label>
					</li>
				{/each}
			</ul>
			<div class="actions">
				<button
					type="button"
					class="primary"
					disabled={chosen.length === 0}
					onclick={acceptSuggestions}
				>
					Add {chosen.length} to the wall
				</button>
				<button type="button" class="quiet" onclick={dismissSuggestions}>Add none</button>
			</div>
		</fieldset>
	{:else}
		<div class="tools">
			<button type="button" aria-pressed={mode === 'corners'} onclick={toggleCorners}>
				{mode === 'corners' ? 'Done' : 'Adjust corners'}
			</button>
			{#if mode === 'corners'}
				<button type="button" disabled={looking} onclick={findCorners}>Find corners for me</button>
			{:else}
				<span class="group" role="group" aria-label="Mark on the photo">
					<span class="group-label">Mark</span>
					{#each MARKABLE as kind (kind)}
						<button
							type="button"
							aria-pressed={mode === kind}
							disabled={!homography}
							onclick={() => {
								notice = null;
								mode = mode === kind ? 'view' : kind;
							}}
						>
							{OBSTRUCTION_NAMES[kind]}
						</button>
					{/each}
					<button type="button" disabled={!homography || looking} onclick={findOnWall}>
						Find them for me
					</button>
				</span>
			{/if}
			<label class="toggle">
				<input type="checkbox" bind:checked={showDesign} />
				Show design
			</label>
			<span class="spacer"></span>
			<label class="button quiet" aria-busy={busy}>
				{busy ? 'Replacing…' : 'Replace photo'}
				<input type="file" accept="image/*" onchange={choose} disabled={busy} />
			</label>
			<button type="button" class="quiet" onclick={remove}>Remove photo</button>
		</div>
	{/if}
	{#if failure}<p class="field-error" role="alert">{failure}</p>{/if}
	<p class="note">
		This is a sketch of where the trim goes, not a rendering. Sizes marked on a photo are
		estimates: check them with a tape measure before cutting.
	</p>
{/if}

<style>
	.empty {
		display: grid;
		justify-items: start;
		gap: 0.6rem;
		padding: 1.5rem;
		border: 1px dashed var(--line-soft);
		border-radius: var(--radius);
	}
	.empty p {
		margin: 0;
		max-width: 34rem;
		color: var(--ink-soft);
	}

	/* A file input styled as a button: the label is the visible control. */
	.button {
		display: inline-block;
		padding: 0.4rem 0.75rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--panel);
		cursor: pointer;
	}
	.button input {
		position: absolute;
		width: 1px;
		height: 1px;
		opacity: 0;
	}
	.button:focus-within {
		outline: 2px solid var(--accent);
		outline-offset: 1px;
	}
	.button.primary {
		border-color: var(--accent);
		background: var(--accent);
		color: white;
		font-weight: 600;
	}
	.button.quiet {
		border-color: transparent;
		background: none;
		color: var(--ink-soft);
	}

	.frame {
		position: relative;
		width: 100%;
		margin-inline: auto;
		background: var(--border);
		border-radius: var(--radius);
		/* Dragging on the photo should move a handle, never select the image. */
		user-select: none;
		-webkit-user-select: none;
	}
	img,
	svg {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		border-radius: var(--radius);
	}
	svg {
		/* Corners that are out of shot sit outside the photo. */
		overflow: visible;
	}
	svg.marking {
		cursor: crosshair;
		touch-action: none;
	}
	.lost {
		position: absolute;
		inset: 0;
		display: grid;
		place-items: center;
		margin: 0;
		padding: 1rem;
		text-align: center;
		color: var(--ink-soft);
	}

	polygon,
	path {
		vector-effect: non-scaling-stroke;
	}
	.paint {
		fill: var(--trim);
		opacity: 0.8;
	}
	.part {
		fill: var(--trim);
		stroke: rgb(60 50 40 / 0.5);
		stroke-width: 0.75;
	}
	.marked,
	.draft {
		fill: rgb(47 93 80 / 0.15);
		stroke: var(--accent);
		stroke-width: 1.5;
		stroke-dasharray: 5 3;
	}
	.draft {
		fill: rgb(47 93 80 / 0.3);
		stroke-dasharray: none;
	}
	.suggested {
		fill: rgb(47 93 80 / 0.25);
		stroke: var(--accent);
		stroke-width: 2;
		cursor: pointer;
	}
	.suggested.off {
		fill: none;
		stroke: white;
		stroke-dasharray: 4 4;
		opacity: 0.8;
	}
	.size.off {
		opacity: 0.5;
	}
	.size {
		text-anchor: middle;
		dominant-baseline: central;
		fill: white;
		font-weight: 700;
		paint-order: stroke;
		stroke: rgb(0 0 0 / 0.65);
		stroke-width: 0.25em;
		pointer-events: none;
	}
	.outline {
		fill: none;
		stroke: var(--accent);
		stroke-width: 2;
	}
	.outline.invalid {
		stroke: var(--danger);
		stroke-dasharray: 6 4;
	}
	.handle {
		cursor: grab;
		touch-action: none;
		outline: none;
	}
	.handle.active {
		cursor: grabbing;
	}
	.grab {
		fill: transparent;
	}
	.dot {
		fill: white;
		stroke: var(--accent);
		stroke-width: 3;
		vector-effect: non-scaling-stroke;
	}
	.handle:focus-visible .dot,
	.handle.active .dot {
		fill: var(--accent);
		stroke: white;
	}

	.hint {
		margin: 0.6rem 0 0;
		padding: 0.5rem 0.75rem;
		border-radius: var(--radius);
		background: var(--warn-bg);
		font-size: 0.9rem;
	}
	/* A button that reads as part of the sentence it follows. */
	.inline {
		margin-left: 0.25rem;
		padding: 0;
		border: none;
		background: none;
		color: var(--accent);
		text-decoration: underline;
	}
	.found {
		margin: 0.75rem 0 0;
		padding: 0.75rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
	}
	.found legend {
		padding: 0 0.3rem;
		font-weight: 600;
	}
	.found p {
		margin: 0;
		font-size: 0.9rem;
		color: var(--ink-soft);
	}
	.found ul {
		display: grid;
		gap: 0.35rem;
		margin: 0.6rem 0;
		padding: 0;
		list-style: none;
	}
	.found label {
		display: flex;
		align-items: baseline;
		gap: 0.45rem;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.4rem;
	}
	.actions .primary {
		border-color: var(--accent);
		background: var(--accent);
		color: white;
		font-weight: 600;
	}
	.actions .primary:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.tools {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.4rem;
		margin-top: 0.75rem;
	}
	.group {
		display: inline-flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.3rem;
		margin-left: 0.4rem;
	}
	.group-label {
		font-size: 0.8rem;
		font-weight: 600;
		color: var(--ink-soft);
	}
	.tools [aria-pressed='true'] {
		border-color: var(--accent);
		background: var(--accent);
		color: white;
	}
	.tools button:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.toggle {
		display: inline-flex;
		align-items: center;
		gap: 0.35rem;
		margin-left: 0.4rem;
		font-size: 0.9rem;
	}
	.spacer {
		flex: 1;
	}
</style>
