/**
 * Who is signed in. The app starts without waiting for this: designing a
 * wall needs no account, and the server may not be reachable at all.
 */
import { ApiError, fetchAccount, signOut as requestSignOut, type Account } from '#lib/api.ts';

type AccountState =
	| { status: 'loading' | 'signed_out' | 'offline' }
	| { status: 'signed_in'; user: Account };

export const account: { current: AccountState } = $state({ current: { status: 'loading' } });

export async function loadAccount() {
	try {
		const user = await fetchAccount();
		account.current = user ? { status: 'signed_in', user } : { status: 'signed_out' };
	} catch (error) {
		const offline = error instanceof ApiError && error.code === 'offline';
		account.current = { status: offline ? 'offline' : 'signed_out' };
	}
}

export function setAccount(user: Account) {
	account.current = { status: 'signed_in', user };
}

/** Updates the balance after an action that spent credits. */
export function setCredits(credits: number) {
	if (account.current.status === 'signed_in') account.current.user.credits = credits;
}

export async function signOut() {
	await requestSignOut().catch(() => {});
	account.current = { status: 'signed_out' };
}
