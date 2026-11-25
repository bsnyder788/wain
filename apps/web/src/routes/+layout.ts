import { loadEngine } from '#lib/engine/index.ts';
import { startSession } from '#lib/session.svelte.ts';

// The app is a client-rendered single-page app: the layout engine is
// WebAssembly and the project lives in the browser. This is also the shape
// the Tauri mobile shell needs.
export const ssr = false;
export const prerender = false;

export async function load() {
	await loadEngine();
	startSession();
}
