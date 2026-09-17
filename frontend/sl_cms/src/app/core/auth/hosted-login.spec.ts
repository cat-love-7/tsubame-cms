import { describe, expect, it, beforeEach, vi } from 'vitest';

import { beginHostedLogin, callbackUrl, challengeFor, forgetSignIn, pendingSignIn } from './hosted-login';

describe('hosted login', () => {
  beforeEach(() => {
    window.sessionStorage.clear();
  });

  // The challenge is what travels; the verifier is what proves the code came back here. A known
  // pair from RFC 7636 is used so the implementation is checked against the specification rather
  // than against itself.
  it('challenges with the SHA-256 of the verifier, base64url', async () => {
    await expect(
      challengeFor('dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk'),
    ).resolves.toBe('E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM');
  });

  it('builds a sign-in address with PKCE, a state and where to come back to', async () => {
    const url = new URL(await beginHostedLogin('https://pool.auth.eu-west-1.amazoncognito.com/login?client_id=abc'));

    expect(url.origin).toBe('https://pool.auth.eu-west-1.amazoncognito.com');
    expect(url.searchParams.get('client_id')).toBe('abc');
    expect(url.searchParams.get('code_challenge_method')).toBe('S256');
    expect(url.searchParams.get('redirect_uri')).toBe(callbackUrl());
    // The challenge is the verifier's digest, and the verifier never leaves the browser.
    const pending = pendingSignIn();
    expect(pending).not.toBeNull();
    await expect(challengeFor(pending!.verifier)).resolves.toBe(url.searchParams.get('code_challenge'));
    expect(url.toString()).not.toContain(pending!.verifier);
    expect(url.searchParams.get('state')).toBe(pending!.state);
  });

  it('forgets the attempt once it is done with', async () => {
    await beginHostedLogin('https://pool.example.com/login?client_id=abc');
    expect(pendingSignIn()).not.toBeNull();

    forgetSignIn();

    expect(pendingSignIn()).toBeNull();
  });

  it('says there is nothing pending when storage is unusable', () => {
    vi.spyOn(window.sessionStorage, 'getItem').mockImplementation(() => {
      throw new Error('no storage');
    });
    expect(pendingSignIn()).toBeNull();
  });
});
