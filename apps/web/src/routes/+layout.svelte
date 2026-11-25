<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	const links = [
		{ href: '/', label: 'Design' },
		{ href: '/settings', label: 'Settings' }
	];
</script>

<svelte:head>
	<title>Wain</title>
	<link
		rel="icon"
		href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'><rect width='16' height='16' rx='3' fill='%232f5d50'/><path d='M3 5h10M3 12h10M5 5v7M8 5v7M11 5v7' stroke='white' stroke-width='1.3'/></svg>"
	/>
</svelte:head>

<header>
	<a class="brand" href="/">Wain</a>
	<nav>
		{#each links as link (link.href)}
			<a href={link.href} aria-current={page.url.pathname === link.href ? 'page' : undefined}>
				{link.label}
			</a>
		{/each}
	</nav>
</header>

<main>
	{@render children()}
</main>

<style>
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.6rem 1rem;
		border-bottom: 1px solid var(--border);
		background: var(--panel);
	}
	.brand {
		font-weight: 800;
		font-size: 1.15rem;
		letter-spacing: 0.02em;
		text-decoration: none;
	}
	nav {
		display: flex;
		gap: 0.25rem;
	}
	nav a {
		padding: 0.3rem 0.7rem;
		border-radius: var(--radius);
		color: var(--ink-soft);
		text-decoration: none;
	}
	nav a[aria-current='page'] {
		color: var(--ink);
		background: var(--surface);
		font-weight: 600;
	}
	main {
		max-width: 80rem;
		margin: 0 auto;
		padding: 1rem;
	}
</style>
