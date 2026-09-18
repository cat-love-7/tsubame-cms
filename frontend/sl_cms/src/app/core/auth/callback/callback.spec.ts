import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, provideRouter, Router } from '@angular/router';

import { AuthService } from '../auth.service';
import { beginHostedLogin, pendingSignIn } from '../hosted-login';
import { AuthCallback } from './callback';

describe('AuthCallback', () => {
  let httpMock: HttpTestingController;
  let auth: AuthService;
  let router: Router;

  beforeEach(() => {
    window.sessionStorage.clear();
  });

  async function create(query: Record<string, string>) {
    TestBed.resetTestingModule();
    await TestBed.configureTestingModule({
      imports: [AuthCallback],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        {
          provide: ActivatedRoute,
          useValue: { snapshot: { queryParamMap: { get: (key: string) => query[key] ?? null } } },
        },
      ],
    }).compileComponents();

    httpMock = TestBed.inject(HttpTestingController);
    auth = TestBed.inject(AuthService);
    router = TestBed.inject(Router);
    auth.clear();
    return TestBed.createComponent(AuthCallback);
  }

  // The happy path: a sign-in this tab started, answered with a code, becomes a session.
  it('exchanges the code for a session and goes home', async () => {
    const signIn = new URL(await beginHostedLogin('https://pool.example.com/login?client_id=abc'));
    const verifier = pendingSignIn()?.verifier as string;
    const fixture = await create({
      code: 'the-code',
      state: signIn.searchParams.get('state') as string,
    });
    const navigate = vi.spyOn(router, 'navigate').mockResolvedValue(true);
    fixture.detectChanges();

    const request = httpMock.expectOne('/api/auth/cognito/exchange');
    expect(request.request.body).toEqual({
      code: 'the-code',
      code_verifier: verifier,
      redirect_uri: `${window.location.origin}/auth/callback`,
    });
    request.flush({ token: 'provider-token', expires_at: '2026-01-01T00:00:00Z' });

    expect(auth.token()).toBe('provider-token');
    expect(navigate).toHaveBeenCalledWith(['/']);
    // The verifier is single-use: it goes with the code.
    expect(pendingSignIn()).toBeNull();
  });

  // A state that is not the one this tab started with means the code is not ours to use.
  it('refuses a code that does not belong to a sign-in this tab started', async () => {
    await beginHostedLogin('https://pool.example.com/login?client_id=abc');
    const fixture = await create({ code: 'the-code', state: 'someone-elses-state' });
    fixture.detectChanges();

    httpMock.expectNone('/api/auth/cognito/exchange');
    expect((fixture.componentInstance.error() as { key: string }).key).toBe('auth.signInNotOurs');
  });

  it('says so when the sign-in was cancelled at the provider', async () => {
    const fixture = await create({ error: 'access_denied' });
    fixture.detectChanges();

    httpMock.expectNone('/api/auth/cognito/exchange');
    expect((fixture.componentInstance.error() as { key: string }).key).toBe('auth.signInCancelled');
  });

  it('reports a refused exchange, and forgets the attempt', async () => {
    const signIn = new URL(await beginHostedLogin('https://pool.example.com/login?client_id=abc'));
    const fixture = await create({
      code: 'the-code',
      state: signIn.searchParams.get('state') as string,
    });
    fixture.detectChanges();

    httpMock
      .expectOne('/api/auth/cognito/exchange')
      .flush('the sign-in could not be completed', { status: 401, statusText: 'Unauthorized' });

    expect(fixture.componentInstance.error()).not.toBeNull();
    expect(auth.token()).toBeNull();
    expect(pendingSignIn()).toBeNull();
  });
});
