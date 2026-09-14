import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { provideTransloco } from '@jsverse/transloco';

import { App } from './app';
import { FALLBACK_LANGUAGE, SUPPORTED_LANGUAGES } from './core/i18n/language';
import { BundledTranslocoLoader } from './core/i18n/transloco.loader';

describe('App', () => {
  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [App],
      providers: [
        provideRouter([]),
        // The root component decides the language, so it needs the catalogs like the app does.
        provideTransloco({
          config: {
            availableLangs: [...SUPPORTED_LANGUAGES],
            defaultLang: FALLBACK_LANGUAGE,
            fallbackLang: FALLBACK_LANGUAGE,
            reRenderOnLangChange: true,
            prodMode: false,
          },
          loader: BundledTranslocoLoader,
        }),
      ],
    }).compileComponents();
  });

  it('should create the app', () => {
    const fixture = TestBed.createComponent(App);
    const app = fixture.componentInstance;
    expect(app).toBeTruthy();
  });

  // `app.html` is only `<router-outlet>`, so there is no <h1> to assert on (the previous
  // version of this test checked for one and could never pass).
  it('should render the router outlet', async () => {
    const fixture = TestBed.createComponent(App);
    await fixture.whenStable();
    const compiled = fixture.nativeElement as HTMLElement;
    expect(compiled.querySelector('router-outlet')).toBeTruthy();
  });
});
