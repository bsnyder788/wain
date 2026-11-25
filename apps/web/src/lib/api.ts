/**
 * The client for the wain server (`/api/v1`). Requests are same-origin, so
 * the session cookie travels with them; in development Vite proxies `/api`
 * to the server.
 *
 * The app works without the server. Only accounts and the AI features need
 * it, so every caller has to cope with `ApiError` of code `offline`.
 */
import type { Project } from '#lib/engine/index.ts';
import type { Account } from '#lib/engine/bindings/Account.ts';
import type { ApiErrorBody } from '#lib/engine/bindings/ApiErrorBody.ts';
import type { CheckoutResponse } from '#lib/engine/bindings/CheckoutResponse.ts';
import type { Pack } from '#lib/engine/bindings/Pack.ts';
import type { PacksResponse } from '#lib/engine/bindings/PacksResponse.ts';
import type { StartResponse } from '#lib/engine/bindings/StartResponse.ts';
import type { SuggestionsResponse } from '#lib/engine/bindings/SuggestionsResponse.ts';

export type { Account } from '#lib/engine/bindings/Account.ts';
export type { Pack } from '#lib/engine/bindings/Pack.ts';
export type { StartResponse } from '#lib/engine/bindings/StartResponse.ts';
export type { Suggestion } from '#lib/engine/bindings/Suggestion.ts';
export type { SuggestionsResponse } from '#lib/engine/bindings/SuggestionsResponse.ts';

export class ApiError extends Error {
	constructor(
		/** HTTP status, or 0 when the server could not be reached. */
		readonly status: number,
		/** The server's error code, e.g. `insufficient_credits`, or `offline`. */
		readonly code: string,
		message: string
	) {
		super(message);
	}
}

const OFFLINE = 'Could not reach the server. Check your connection and try again.';

async function request<T>(path: string, init?: RequestInit): Promise<T> {
	let response: Response;
	try {
		response = await fetch(`/api/v1${path}`, { credentials: 'same-origin', ...init });
	} catch {
		throw new ApiError(0, 'offline', OFFLINE);
	}
	const body: unknown = await response.json().catch(() => undefined);
	if (response.ok) return body as T;

	// The server always answers errors with an `ApiErrorBody`. Anything else
	// came from a proxy in front of a server that is not running.
	const error = body as Partial<ApiErrorBody> | undefined;
	if (typeof error?.error === 'string' && typeof error.message === 'string') {
		throw new ApiError(response.status, error.error, error.message);
	}
	throw new ApiError(0, 'offline', OFFLINE);
}

function post<T>(path: string, body: unknown): Promise<T> {
	return request<T>(path, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify(body)
	});
}

/** The signed-in account, or `undefined` when nobody is signed in. */
export async function fetchAccount(): Promise<Account | undefined> {
	try {
		return await request<Account>('/me');
	} catch (error) {
		if (error instanceof ApiError && error.status === 401) return undefined;
		throw error;
	}
}

/** Emails a sign-in code to the address. */
export function startSignIn(email: string): Promise<StartResponse> {
	return post('/auth/start', { email });
}

/** Exchanges the emailed code for a session. */
export function verifyCode(challenge_id: string, code: string): Promise<Account> {
	return post('/auth/verify', { challenge_id, code });
}

export function signOut(): Promise<unknown> {
	return post('/auth/signout', {});
}

/** Asks the AI for designs to suit one wall, given its photo. Costs a credit. */
export function suggestDesigns(
	project: Project,
	wallIndex: number,
	photo: Blob
): Promise<SuggestionsResponse> {
	const form = new FormData();
	form.set('project', JSON.stringify(project));
	form.set('wall', String(wallIndex));
	form.set('image', photo, 'wall.jpg');
	return request('/suggestions', { method: 'POST', body: form });
}

/** The credit packs on sale. Empty when this server does not sell credits. */
export async function fetchPacks(): Promise<Pack[]> {
	return (await request<PacksResponse>('/billing/packs')).packs;
}

/** Starts a purchase and returns the payment page to send the browser to. */
export async function startCheckout(pack: string): Promise<string> {
	return (await post<CheckoutResponse>('/billing/checkout', { pack })).url;
}

/** A price in a currency's smallest unit, as text: 500 and `usd` is $5.00. */
export function formatMoney(amount: number, currency: string): string {
	const format = new Intl.NumberFormat(undefined, {
		style: 'currency',
		currency: currency.toUpperCase()
	});
	// Most currencies have two decimal places; some, like the yen, have none.
	const digits = format.resolvedOptions().maximumFractionDigits ?? 2;
	return format.format(amount / 10 ** digits);
}
