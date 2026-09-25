import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { provideRouter } from '@angular/router';
import { HttpTestingController } from '@angular/common/http/testing';

import { TypedFixture } from 'app/core/testing/fixture';
import { Login } from './login';

describe('Login', () => {
  let component: Login;
  let fixture: TypedFixture<Login>;
  let httpMock: HttpTestingController;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [Login],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    }).compileComponents();

    httpMock = TestBed.inject(HttpTestingController);
    fixture = TestBed.createComponent(Login);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  // A deployment that signs users in elsewhere offers a button, not a link: the address needs a
  // fresh PKCE challenge and state, which only this browser can produce.
  it('sends the browser to the provider with a challenge and a state', async () => {
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: false,
      password_reset: 'temporary',
      login_url: 'https://pool.example.com/login?client_id=abc',
    });
    await fixture.whenStable();
    fixture.detectChanges();

    const url = new URL((await component.providerSignInUrl()) as string);
    expect(url.origin).toBe('https://pool.example.com');
    expect(url.searchParams.get('code_challenge_method')).toBe('S256');
    expect(url.searchParams.get('state')).toBeTruthy();
    expect(url.searchParams.get('redirect_uri')).toBe(`${window.location.origin}/auth/callback`);
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  /** A throttled sign-in explains the wait instead of blaming the password. */
  it('turns a 429 into advice about waiting, with the seconds filled in', async () => {
    component.submit();
    const request = httpMock.expectOne('/api/auth/login');
    request.flush('too many failed attempts; try again in 600 seconds', {
      status: 429,
      statusText: 'Too Many Requests',
      headers: { 'Retry-After': '600' },
    });

    expect(component.busy()).toBe(false);
    // The component holds the key and the values; the wording is the catalog's business, and
    // the catalog test is what checks it reads well.
    expect(component.error()).toEqual({ key: 'auth.tooManyAttempts', params: { seconds: 600 } });

    // And the wait is really on screen: a placeholder that is never substituted would leave
    // the message readable but useless.
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.nativeElement.textContent).toContain('600');
    expect(fixture.nativeElement.textContent).not.toContain('{{seconds}}');
  });

  it('shows where to sign in when the deployment leaves it to an identity provider', async () => {
    // The component asks what the deployment can do when it is created, so the first request
    // is that question.
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: false,
      password_reset: 'temporary',
      image_upload: 'presigned',
    });
    await fixture.whenStable();
    fixture.detectChanges();

    const password = fixture.nativeElement.querySelector('input[type="password"]');
    expect(password).toBeNull();

    const message: HTMLElement | null = fixture.nativeElement.querySelector('.elsewhere');
    expect(message?.textContent).toContain('identity provider');
  });

  it('sends the user to the provider when the deployment names a sign-in page', async () => {
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: false,
      password_reset: 'temporary',
      image_upload: 'presigned',
      login_url: 'https://cms.auth.eu-west-1.amazoncognito.com/login?client_id=abc',
    });
    await fixture.whenStable();
    fixture.detectChanges();

    // A button, not a link: the address needs a fresh challenge and state, which an `href` cannot
    // carry. What it produces is checked in the test above.
    const button: HTMLButtonElement | null =
      fixture.nativeElement.querySelector('button.full-width');
    expect(button?.textContent).toContain('Go to sign in');
  });

  it('can be switched to Japanese, and remembers the choice', async () => {
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: true,
      password_reset: 'link',
      image_upload: 'proxied',
    });
    await fixture.whenStable();
    fixture.detectChanges();

    // The switch is on the sign-in screen on purpose: somebody who cannot read the language the
    // browser guessed is exactly the person who cannot look behind a sign-in for a setting.
    const japanese = [...fixture.nativeElement.querySelectorAll('button')].find(
      (button: HTMLButtonElement) => button.textContent?.includes('日本語'),
    );
    expect(japanese).toBeTruthy();
    japanese!.click();
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.nativeElement.textContent).toContain('パスワード');
    expect(localStorage.getItem('tsubame.language')).toBe('ja');
  });
});
