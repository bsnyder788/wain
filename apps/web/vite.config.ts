import { defineConfig } from 'vitest/config';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';

/** Where wain-server listens in development (its default port). */
const API = 'http://127.0.0.1:8787';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// A single-page app: every route is served by the fallback page.
			adapter: adapter({ fallback: 'index.html' })
		})
	],
	// Lets the dev and preview servers answer on a Tailscale name, for trying
	// the app on a phone over HTTPS with `tailscale serve`. Vite otherwise
	// refuses requests whose Host header is not localhost.
	// Listening on 127.0.0.1 matters: by default Vite binds only the IPv6
	// loopback (::1), which `tailscale serve` does not proxy to.
	//
	// `/api` goes to wain-server, so the browser sees one origin and the
	// session cookie just works. Without the server running the app still
	// loads; only accounts and AI features are unavailable.
	server: { host: '127.0.0.1', allowedHosts: ['.ts.net'], proxy: { '/api': API } },
	preview: { host: '127.0.0.1', allowedHosts: ['.ts.net'], proxy: { '/api': API } },
	test: {
		expect: { requireAssertions: true },
		projects: [
			{
				extends: './vite.config.ts',
				test: {
					name: 'server',
					environment: 'node',
					include: ['src/**/*.{test,spec}.{js,ts}'],
					exclude: ['src/**/*.svelte.{test,spec}.{js,ts}']
				}
			}
		]
	}
});
