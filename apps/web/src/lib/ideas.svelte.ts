/**
 * AI design suggestions, kept per wall. They cost a credit, so they are
 * saved on the device and survive a reload or a change of tab.
 */
import type { SuggestionsResponse } from '#lib/api.ts';

const KEY = 'wain.ideas.v1';

function read(): Record<string, SuggestionsResponse> {
	try {
		return JSON.parse(localStorage.getItem(KEY) ?? '{}');
	} catch {
		return {};
	}
}

/** Suggestions by wall id. */
export const ideas: Record<string, SuggestionsResponse> = $state(read());

export function saveIdeas(wallId: string, response: SuggestionsResponse) {
	ideas[wallId] = response;
	try {
		localStorage.setItem(KEY, JSON.stringify(ideas));
	} catch {
		// Storage can be full or disabled; the suggestions still show for now.
	}
}
