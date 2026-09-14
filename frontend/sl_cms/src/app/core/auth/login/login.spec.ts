import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { provideRouter } from '@angular/router';
import { HttpTestingController } from '@angular/common/http/testing';

import { Login } from './login';

describe('Login', () => {
  let component: Login;
  let fixture: ComponentFixture<Login>;
  let httpMock: HttpTestingController;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [Login],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    })
    .compileComponents();

    httpMock = TestBed.inject(HttpTestingController);
    fixture = TestBed.createComponent(Login);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  /** A throttled sign-in explains the wait instead of blaming the password. */
  it('turns a 429 into advice about waiting', () => {
    component.submit();
    const request = httpMock.expectOne('/api/auth/login');
    request.flush('too many failed attempts; try again in 600 seconds', {
      status: 429,
      statusText: 'Too Many Requests',
      headers: { 'Retry-After': '600' },
    });

    expect(component.busy()).toBe(false);
    expect(component.error()).toContain('ロックされています');
    expect(component.error()).toContain('10 分後');
  });

  it('shows where to sign in when the deployment leaves it to an identity provider', async () => {
    // The component asks what the deployment can do when it is created, so the first request
    // is that question.
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: false,
      password_reset_links: false,
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
      password_reset_links: false,
      image_upload: 'presigned',
      login_url: 'https://cms.auth.eu-west-1.amazoncognito.com/login?client_id=abc',
    });
    await fixture.whenStable();
    fixture.detectChanges();

    const link: HTMLAnchorElement | null = fixture.nativeElement.querySelector('a.full-width');
    expect(link?.getAttribute('href')).toBe(
      'https://cms.auth.eu-west-1.amazoncognito.com/login?client_id=abc',
    );
  });
});
