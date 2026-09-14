import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivatedRoute, provideRouter, Router } from '@angular/router';

import { AuthService } from '../auth.service';
import { PasswordReset } from './password-reset';

describe('PasswordReset', () => {
  let fixture: ComponentFixture<PasswordReset>;
  let httpMock: HttpTestingController;
  let auth: AuthService;
  let router: Router;

  async function create(token: string) {
    TestBed.resetTestingModule();
    await TestBed.configureTestingModule({
      imports: [PasswordReset],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        {
          provide: ActivatedRoute,
          useValue: { snapshot: { queryParamMap: { get: () => token } } },
        },
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

    // The replacement token is adopted, so the screen the user lands on works.
    expect(auth.token()).toBe('fresh-token');
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
