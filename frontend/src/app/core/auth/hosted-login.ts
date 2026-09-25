/**
 * Signing in at the identity provider's own page.
 *
 * A deployment that does not handle passwords sends the browser to the provider's hosted sign-in
 * page and gets it back with an authorization code. Two things make that safe for a client with no
 * secret:
 *
 * - **PKCE**: a random verifier is generated here, and only its SHA-256 challenge travels in the
 *   sign-in URL. The verifier stays in this browser and is sent with the code when it comes back,
 *   so a code intercepted on the way is useless without it.
 * - **state**: a random value echoed by the provider, which is what tells this screen that the
 *   answer belongs to the sign-in it started rather than to one someone else arranged.
 *
 * Both live in `sessionStorage` under this tab's key: they are single-use and belong to one
 * sign-in attempt, so a second tab starting one must not overwrite them.
 */

const VERIFIER_KEY = 'tsubame.pkce_verifier';
const STATE_KEY = 'tsubame.pkce_state';

/** Where the provider sends the browser back to, which has to be registered on the client. */
export function callbackUrl(): string {
  return `${window.location.origin}/auth/callback`;
}

/** A random string in the alphabet PKCE and `state` are allowed to use. */
function randomValue(bytes = 32): string {
  const buffer = new Uint8Array(bytes);
  crypto.getRandomValues(buffer);
  return base64Url(buffer);
}

/** The URL-safe alphabet without padding, which is what PKCE asks for. */
function base64Url(buffer: Uint8Array): string {
  let binary = '';
  for (const byte of buffer) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** The challenge for a verifier: SHA-256, base64url. */
export async function challengeFor(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier));
  return base64Url(new Uint8Array(digest));
}

/** What the callback needs to prove the answer is ours. */
export interface PendingSignIn {
  verifier: string;
  state: string;
}

/**
 * Start a sign-in: remember the verifier and state, and hand back the address to visit.
 *
 * `loginUrl` is the deployment's sign-in page (from `GET /auth/capabilities`), which already names
 * the client, the response type and the scope. What is added here is what only this browser knows.
 */
export async function beginHostedLogin(loginUrl: string): Promise<string> {
  const verifier = randomValue();
  const state = randomValue(16);
  const challenge = await challengeFor(verifier);
  try {
    window.sessionStorage.setItem(VERIFIER_KEY, verifier);
    window.sessionStorage.setItem(STATE_KEY, state);
  } catch {
    // A browser that refuses storage cannot complete this flow: without the verifier the code
    // cannot be exchanged, and the callback will say so rather than hanging.
  }

  const url = new URL(loginUrl, window.location.origin);
  url.searchParams.set('redirect_uri', callbackUrl());
  url.searchParams.set('code_challenge', challenge);
  url.searchParams.set('code_challenge_method', 'S256');
  url.searchParams.set('state', state);
  return url.toString();
}

/** The verifier and state this browser is waiting with, if a sign-in is in flight. */
export function pendingSignIn(): PendingSignIn | null {
  try {
    const verifier = window.sessionStorage.getItem(VERIFIER_KEY);
    const state = window.sessionStorage.getItem(STATE_KEY);
    return verifier && state ? { verifier, state } : null;
  } catch {
    return null;
  }
}

/** Forget the attempt: the code is single-use, so the verifier goes with it. */
export function forgetSignIn(): void {
  try {
    window.sessionStorage.removeItem(VERIFIER_KEY);
    window.sessionStorage.removeItem(STATE_KEY);
  } catch {
    // Nothing to forget.
  }
}
