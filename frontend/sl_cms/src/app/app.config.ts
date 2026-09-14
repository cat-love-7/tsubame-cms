import { provideHttpClient, withInterceptors } from '@angular/common/http';
import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';
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
