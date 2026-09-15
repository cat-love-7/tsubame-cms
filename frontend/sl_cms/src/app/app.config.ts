import { provideHttpClient, withInterceptors } from '@angular/common/http';
import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';
import { MAT_TOOLTIP_DEFAULT_OPTIONS } from '@angular/material/tooltip';
import { provideRouter } from '@angular/router';
import { provideTransloco } from '@jsverse/transloco';

import { routes } from './app.routes';
import { authInterceptor } from './core/auth/auth.interceptor';
import { FALLBACK_LANGUAGE, SUPPORTED_LANGUAGES } from './core/i18n/language';
import { BundledTranslocoLoader } from './core/i18n/transloco.loader';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes),
    // Icon buttons explain themselves on hover, after a moment: a tooltip that appears the instant
    // the pointer crosses one flashes while the reader is on their way to another.
    {
      provide: MAT_TOOLTIP_DEFAULT_OPTIONS,
      useValue: {
        showDelay: 300,
        hideDelay: 100,
        // A tooltip is a hint, not a control: Material leaves it interactive (one may hold its own
        // buttons), which means the hint from the icon just pressed can swallow the press on the
        // next one in a row.
        disableTooltipInteractivity: true,
      },
    },
    // `provideHttpClient` is what makes the auth interceptor apply to every request.
    // Angular 22's default backend (fetch) is what we want; the migration to v22 added
    // `withXhr()` only to preserve the v21 behaviour, and nothing here needs XHR - no
    // upload progress, and `observe: 'response'` works on both.
    provideHttpClient(withInterceptors([authInterceptor])),
    provideTransloco({
      config: {
        availableLangs: [...SUPPORTED_LANGUAGES],
        defaultLang: FALLBACK_LANGUAGE,
        // A key that has not been translated yet reads as English rather than as a key.
        fallbackLang: FALLBACK_LANGUAGE,
        // The templates read the active language, so switching has to re-render them.
        reRenderOnLangChange: true,
        prodMode: false,
      },
      loader: BundledTranslocoLoader,
    }),
  ]
};
