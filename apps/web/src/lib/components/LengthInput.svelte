<script lang="ts">
	import { formatLength, parseLength, type Length, type UnitPrefs } from '#lib/engine/index.ts';

	type Props = {
		label: string;
		value: Length;
		units: UnitPrefs;
		hint?: string;
	};
	let { label, value = $bindable(), units, hint }: Props = $props();
	const id = $props.id();

	// While the user is typing, `draft` holds their text. Otherwise the field
	// shows the stored value in the preferred units.
	let draft = $state<string | null>(null);
	let error = $state<string | null>(null);
	const shown = $derived(draft ?? formatLength(value, units));

	function commit() {
		if (draft === null) return;
		const result = parseLength(draft, units.system);
		if ('error' in result) {
			error = result.error;
			return;
		}
		value = result.ok;
		draft = null;
		error = null;
	}

	function onkeydown(event: KeyboardEvent & { currentTarget: HTMLInputElement }) {
		if (event.key === 'Enter') {
			commit();
		} else if (event.key === 'Escape') {
			draft = null;
			error = null;
		}
	}
</script>

<div class="field">
	<label class="field-label" for={id}>{label}</label>
	<input
		{id}
		type="text"
		autocomplete="off"
		spellcheck="false"
		value={shown}
		aria-invalid={error !== null}
		aria-describedby={error || hint ? `${id}-note` : undefined}
		oninput={(event) => (draft = event.currentTarget.value)}
		onblur={commit}
		{onkeydown}
	/>
	{#if error}
		<span id="{id}-note" class="field-error" role="alert">{error}</span>
	{:else if hint}
		<span id="{id}-note" class="field-hint">{hint}</span>
	{/if}
</div>
