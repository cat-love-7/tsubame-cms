import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';

import { AuthService, CurrentUser } from './auth.service';

function account(overrides: Partial<CurrentUser> = {}): CurrentUser {
  return {
    id: 'user-1',
    username: 'editor@example.com',
    email: null,
    is_admin: false,
    is_active: true,
    permission: { can_view: true, can_edit: true, can_publish: false },
    created_at: '2026-01-01T00:00:00Z',
    last_login: null,
    collection_permissions: {},
    single_page_permissions: {},
    ...overrides,
  };
}

describe('AuthService', () => {
  let auth: AuthService;
  let httpMock: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    });
    auth = TestBed.inject(AuthService);
    httpMock = TestBed.inject(HttpTestingController);
    auth.clear();
    localStorage.clear();
  });

  // Somebody following a password reset link has a token and nothing else: every permission
  // question is answered from the account's record, so it has to be read back - otherwise the
  // CMS shows them the screens of a viewer (no permission is true for an unknown account).
  it('reads the account when a token is remembered but the record is not', () => {
    auth.replaceToken('remembered-token');
    expect(auth.isAuthenticated()).toBe(true);
    expect(auth.canEdit()).toBe(false);

    let read: CurrentUser | null | undefined;
    auth.loadUserIfMissing().subscribe((user) => (read = user));

    httpMock.expectOne('/api/auth/me').flush(account());
    expect(read?.username).toBe('editor@example.com');
    expect(auth.user()?.username).toBe('editor@example.com');
    // The permissions are known now, which is the whole point of reading it.
    expect(auth.canEdit()).toBe(true);
  });

  // A reset link belongs to one account: completing it here while another was signed in puts a
  // *different* session in place, and the record that went with the old one has to go with it.
  it('forgets the previous account when a reset signs in as another', () => {
    auth.login('first@example.com', 'password').subscribe();
    httpMock.expectOne('/api/auth/login').flush({
      token: 'first-token',
      expires_at: '2026-12-31T00:00:00Z',
      user: account({ username: 'first@example.com' }),
    });
    expect(auth.canEdit()).toBe(true);

    auth.completePasswordReset('link-token', 'chosen-password').subscribe();
    httpMock
      .expectOne('/api/auth/password-reset')
      .flush({ token: 'second-token', expires_at: '2026-12-31T00:00:00Z' });
    // The account read fails: nothing may be left describing the previous session.
    httpMock.expectOne('/api/auth/me').flush('nope', { status: 500, statusText: 'Server Error' });

    expect(auth.token()).toBe('second-token');
    expect(auth.user()).toBeNull();
    expect(auth.canEdit()).toBe(false);
    // Which is what makes the shell ask again rather than trust a stale record.
    let read: CurrentUser | null | undefined;
    auth.loadUserIfMissing().subscribe((user) => (read = user));
    httpMock.expectOne('/api/auth/me').flush(account({ username: 'second@example.com' }));
    expect(read?.username).toBe('second@example.com');
  });

  it('asks nothing when the record is already known, or when nobody is signed in', () => {
    // Nobody signed in: there is nothing to read.
    auth.loadUserIfMissing().subscribe();
    httpMock.expectNone('/api/auth/me');

    // Signed in, with the record beside the token: enough to answer with.
    auth.login('known@example.com', 'password').subscribe();
    httpMock.expectOne('/api/auth/login').flush({
      token: 'remembered-token',
      expires_at: '2026-12-31T00:00:00Z',
      user: account({ username: 'known@example.com' }),
    });

    let read: CurrentUser | null | undefined;
    auth.loadUserIfMissing().subscribe((user) => (read = user));
    expect(read).toBeNull();
    httpMock.expectNone('/api/auth/me');
  });
});
