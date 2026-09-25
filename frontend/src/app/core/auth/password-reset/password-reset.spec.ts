import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivatedRoute, provideRouter, Router } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';

import { AuthService } from '../auth.service';
import { PasswordReset } from './password-reset';

describe('PasswordReset', () => {
  let fixture: ComponentFixture<PasswordReset>;
  let httpMock: HttpTestingController;
  let auth: AuthService;
  let router: Router;
  let route: ReturnType<typeof stubActivatedRoute>;

  async function create(token: string) {
    TestBed.resetTestingModule();
    route = stubActivatedRoute({}, { token });
    await TestBed.configureTestingModule({
      imports: [PasswordReset],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
      ],
    }).compileComponents();

    httpMock = TestBed.inject(HttpTestingController);
    auth = TestBed.inject(AuthService);
    router = TestBed.inject(Router);
    auth.clear();
    fixture = TestBed.createComponent(PasswordReset);
    await fixture.whenStable();
  }

  it('sets the password with the token from the link and signs the caller in', async () => {
    await create('user-1.0.1758000000.abc123');
    const component = fixture.componentInstance;

    component.next = 'chosen-password';
    component.repeated = 'chosen-password';
    const navigate = vi.spyOn(router, 'navigate').mockResolvedValue(true);
    component.save();

    const request = httpMock.expectOne('/api/auth/password-reset');
    expect(request.request.body).toEqual({
      token: 'user-1.0.1758000000.abc123',
      new_password: 'chosen-password',
    });
    request.flush({ token: 'fresh-token', expires_at: '2026-09-13T12:00:00Z' });

    // The replacement token is adopted, so the screen the user lands on works...
    expect(auth.token()).toBe('fresh-token');
    // ...and the account is read back, because somebody following a reset link has never signed
    // in on this browser: without this the CMS would know a token and nothing about who it is.
    httpMock.expectOne('/api/auth/me').flush({
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
    });
    expect(auth.user()?.username).toBe('editor@example.com');
    expect(navigate).toHaveBeenCalledWith(['/']);
  });

  // Opening another link on this screen is the same route with a different query, so the router
  // keeps the component and only the parameters change. The token sent has to be the new one: with
  // a token read once, a second link would reset nothing and say "already used".
  it('sends the token the address names now, not the one the screen opened with', async () => {
    await create('spent-token');
    const component = fixture.componentInstance;

    route.navigateQuery({ token: 'the-new-token' });
    component.next = 'chosen-password';
    component.repeated = 'chosen-password';
    component.save();

    const request = httpMock.expectOne('/api/auth/password-reset');
    expect(request.request.body).toEqual({
      token: 'the-new-token',
      new_password: 'chosen-password',
    });
  });

  // The password *was* changed: a failure to read the account back is not a failed reset, and
  // telling the reader it was would send them back to a link that no longer works.
  it('still signs the caller in when reading the account back fails', async () => {
    await create('user-1.0.1758000000.abc123');
    const component = fixture.componentInstance;
    component.next = 'chosen-password';
    component.repeated = 'chosen-password';
    const navigate = vi.spyOn(router, 'navigate').mockResolvedValue(true);
    component.save();

    httpMock
      .expectOne('/api/auth/password-reset')
      .flush({ token: 'fresh-token', expires_at: '2026-09-13T12:00:00Z' });
    // The account read is refused for whatever reason.
    httpMock.expectOne('/api/auth/me').flush('nope', { status: 500, statusText: 'Server Error' });

    expect(auth.token()).toBe('fresh-token');
    expect(component.error()).toBeNull();
    expect(navigate).toHaveBeenCalledWith(['/']);
  });

  it('shows what the server said when the link is no longer usable', async () => {
    await create('spent-token');
    const component = fixture.componentInstance;

    component.next = 'chosen-password';
    component.repeated = 'chosen-password';
    component.save();
    httpMock
      .expectOne('/api/auth/password-reset')
      .flush('this password reset link has already been used or is no longer valid', {
        status: 403,
        statusText: 'Forbidden',
      });

    // The link itself was refused, so the server's own answer is what the screen shows.
    expect(component.error()).toEqual({
      text: 'this password reset link has already been used or is no longer valid',
    });
    expect(component.busy()).toBe(false);
  });

  it('refuses a password that is too short or mistyped before calling the server', async () => {
    await create('some-token');
    const component = fixture.componentInstance;

    component.next = 'short';
    component.repeated = 'short';
    component.save();
    expect(component.error()).toEqual({ key: 'auth.passwordTooShort' });

    component.next = 'long-enough';
    component.repeated = 'not-the-same';
    component.save();
    expect(component.error()).toEqual({ key: 'auth.passwordMismatch' });

    httpMock.verify();
  });
});
