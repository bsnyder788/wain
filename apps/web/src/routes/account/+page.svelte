<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { account, loadAccount, setAccount, signOut } from '#lib/account.svelte.ts';
	import {
		ApiError,
		fetchPacks,
		formatMoney,
		startCheckout,
		startSignIn,
		verifyCode,
		type Pack
	} from '#lib/api.ts';

	// Not named `state`: that would shadow the `$state` rune.
	const who = $derived(account.current);

	let email = $state('');
	let code = $state('');
	// Set once a code has been sent; identifies which code is being checked.
	let challenge = $state<string | null>(null);
	let devCode = $state<string | null>(null);
	let busy = $state(false);
	let failure = $state<string | null>(null);

	async function run(action: () => Promise<void>) {
		busy = true;
		failure = null;
		try {
			await action();
		} catch (error) {
			failure = error instanceof ApiError ? error.message : 'Something went wrong. Try again.';
		} finally {
			busy = false;
		}
	}

	function sendCode(event: SubmitEvent) {
		event.preventDefault();
		run(async () => {
			const started = await startSignIn(email);
			challenge = started.challenge_id;
			devCode = started.dev_code;
			code = started.dev_code ?? '';
		});
	}

	function checkCode(event: SubmitEvent) {
		event.preventDefault();
		run(async () => {
			if (!challenge) return;
			setAccount(await verifyCode(challenge, code));
			challenge = null;
			code = '';
		});
	}

	function startOver() {
		challenge = null;
		code = '';
		failure = null;
	}

	// ---- Buying credits ------------------------------------------------------

	let packs = $state<Pack[]>([]);
	// How the browser came back from the payment page, if it just did.
	let purchase = $state<'success' | 'cancelled' | null>(null);

	onMount(() => {
		fetchPacks()
			.then((found) => (packs = found))
			.catch(() => {});

		const outcome = page.url.searchParams.get('purchase');
		if (outcome !== 'success' && outcome !== 'cancelled') return;
		purchase = outcome;
		// Drop the marker so a reload does not show the message again.
		history.replaceState(history.state, '', page.url.pathname);
		if (outcome !== 'success') return;

		// The credits are added when the payment provider confirms, which can
		// be a moment after the browser returns. Check a few times.
		let checks = 0;
		const timer = setInterval(() => {
			void loadAccount();
			if (++checks >= 4) clearInterval(timer);
		}, 2000);
		void loadAccount();
		return () => clearInterval(timer);
	});

	function buy(pack: Pack) {
		run(async () => {
			window.location.href = await startCheckout(pack.id);
		});
	}
</script>

<div class="account">
	<h1>Account</h1>

	<section class="panel">
		{#if who.status === 'signed_in'}
			<h2>{who.user.email}</h2>
			<p class="balance"><strong>{who.user.credits}</strong> credits</p>
			<p class="note">
				Credits pay for AI features: one for each set of design suggestions. Designing, drawings
				and cut lists are free and need no credits.
			</p>
			{#if purchase === 'success'}
				<p class="notice" role="status">
					Thank you. Your credits are added as soon as the payment is confirmed, usually within a
					few seconds.
				</p>
			{:else if purchase === 'cancelled'}
				<p class="notice" role="status">The purchase was cancelled and you were not charged.</p>
			{/if}

			{#if packs.length > 0}
				<h3>Buy credits</h3>
				<div class="packs">
					{#each packs as pack (pack.id)}
						<button type="button" onclick={() => buy(pack)} disabled={busy}>
							<strong>{pack.credits} credits</strong>
							<span>{formatMoney(pack.amount, pack.currency)}</span>
						</button>
					{/each}
				</div>
				<p class="note">Payment is taken on a secure page run by Stripe. Credits do not expire.</p>
			{/if}

			<button type="button" class="quiet signout" onclick={signOut}>Sign out</button>
		{:else if who.status === 'offline'}
			<h2>Accounts are unavailable</h2>
			<p class="note">
				The server cannot be reached right now. You can keep designing; your work is saved on this
				device.
			</p>
		{:else if who.status === 'loading'}
			<p class="note">Checking…</p>
		{:else if challenge}
			<h2>Enter your code</h2>
			<p class="note">We sent a six-digit code to <strong>{email}</strong>. It works for ten minutes.</p>
			{#if devCode}
				<p class="dev">Development mode: no email was sent. Your code is <strong>{devCode}</strong>.</p>
			{/if}
			<form onsubmit={checkCode}>
				<label class="field">
					<span class="field-label">Code</span>
					<input
						type="text"
						inputmode="numeric"
						autocomplete="one-time-code"
						pattern="[0-9]*"
						maxlength="6"
						required
						bind:value={code}
					/>
				</label>
				<div class="actions">
					<button type="submit" class="primary" disabled={busy}>Sign in</button>
					<button type="button" class="quiet" onclick={startOver}>Use a different email</button>
				</div>
			</form>
		{:else}
			<h2>Sign in or create an account</h2>
			<p class="note">
				Enter your email and we will send you a code. There is no password. New accounts start with
				10 free credits for AI design suggestions.
			</p>
			<form onsubmit={sendCode}>
				<label class="field">
					<span class="field-label">Email</span>
					<input type="email" autocomplete="email" required bind:value={email} />
				</label>
				<div class="actions">
					<button type="submit" class="primary" disabled={busy}>Email me a code</button>
				</div>
			</form>
		{/if}
		{#if failure}<p class="field-error" role="alert">{failure}</p>{/if}
	</section>
</div>

<style>
	.account {
		max-width: 28rem;
		display: grid;
		gap: 1rem;
	}
	form {
		display: grid;
		gap: 0.75rem;
		margin-top: 0.75rem;
	}
	input {
		width: 100%;
		padding: 0.5rem 0.6rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--panel);
		font-size: 1rem;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}
	.primary {
		border-color: var(--accent);
		background: var(--accent);
		color: white;
		font-weight: 600;
	}
	.balance {
		margin: 0.25rem 0 0;
		font-size: 1.1rem;
	}
	.dev {
		margin: 0.6rem 0 0;
		padding: 0.5rem 0.75rem;
		border-radius: var(--radius);
		background: var(--warn-bg);
		font-size: 0.9rem;
	}
	.notice {
		margin: 0.75rem 0 0;
		padding: 0.5rem 0.75rem;
		border-radius: var(--radius);
		background: var(--warn-bg);
		font-size: 0.9rem;
	}
	h3 {
		margin: 1.25rem 0 0.5rem;
	}
	.packs {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}
	.packs button {
		display: grid;
		gap: 0.1rem;
		min-width: 8rem;
		padding: 0.6rem 0.9rem;
		text-align: left;
	}
	.packs span {
		color: var(--ink-soft);
	}
	.signout {
		margin-top: 1.25rem;
		padding-left: 0;
	}
</style>
