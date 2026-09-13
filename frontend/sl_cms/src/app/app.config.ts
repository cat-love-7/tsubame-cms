import { provideHttpClient, withInterceptors } from '@angular/common/http';
import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';
import { provideRouter } from '@angular/router';

import { routes } from './app.routes';
import { authInterceptor } from './core/auth/auth.interceptor';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes),
    // `provideHttpClient` is what makes the auth interceptor apply to every request.
    // Angular 22's default backend (fetch) is what we want; the migration to v22 added
    // `withXhr()` only to preserve the v21 behaviour, and nothing here needs XHR - no
    // upload progress, and `observe: 'response'` works on both.
    provideHttpClient(withInterceptors([authInterceptor])),
  ]
};
