import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { Title } from '@angular/platform-browser';
import { provideRouter } from '@angular/router';

import { TypedFixture } from 'app/core/testing/fixture';
import { App } from './app';

describe('App', () => {
  let httpMock: HttpTestingController;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [App],
      // Transloco comes from the test environment (`src/test-providers.ts`), like every spec.
      providers: [provideRouter([]), provideHttpClient(), provideHttpClientTesting()],
    }).compileComponents();
    httpMock = TestBed.inject(HttpTestingController);
  });

  /** The one request the root makes: what the deployment can do (see the constructor). */
  function answerCapabilities(siteName?: string): void {
    httpMock.expectOne('/api/auth/capabilities').flush({
      password_login: true,
      password_reset: 'link',
      image_upload: 'proxied',
      site_name: siteName ?? null,
    });
  }

  it('should create the app', () => {
    const fixture: TypedFixture<App> = TestBed.createComponent(App);
    answerCapabilities();
    const app = fixture.componentInstance;
    expect(app).toBeTruthy();
  });

  // `app.html` is only `<router-outlet>`, so there is no <h1> to assert on (the previous
  // version of this test checked for one and could never pass).
  it('should render the router outlet', async () => {
    const fixture: TypedFixture<App> = TestBed.createComponent(App);
    answerCapabilities();
    await fixture.whenStable();
    const compiled = fixture.nativeElement;
    expect(compiled.querySelector('router-outlet')).toBeTruthy();
  });

  // The tab is where "which site is this?" is answered for someone who has several CMSs open, and
  // it is settled here rather than by a screen: the sign-in screen has no shell, and a page opened
  // from a bookmark goes to the shell only after it has already been looked at.
  it('puts the deployment in the tab title', async () => {
    const fixture: TypedFixture<App> = TestBed.createComponent(App);
    answerCapabilities('サンプル管理画面(dev)');
    await fixture.whenStable();

    expect(TestBed.inject(Title).getTitle()).toBe('サンプル管理画面(dev) — Tsubame');
  });

  // A deployment that never named itself keeps the title `index.html` shipped, rather than an
  // empty one where a name would have gone.
  it('leaves the tab title to the product when the deployment did not name itself', async () => {
    const fixture: TypedFixture<App> = TestBed.createComponent(App);
    answerCapabilities();
    await fixture.whenStable();

    expect(TestBed.inject(Title).getTitle()).toBe('Tsubame');
  });
});
