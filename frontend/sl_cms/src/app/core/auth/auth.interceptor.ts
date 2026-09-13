import { HttpErrorResponse, HttpInterceptorFn } from '@angular/common/http';
import { inject } from '@angular/core';
import { Router } from '@angular/router';
import { catchError, throwError } from 'rxjs';

import { AuthService } from './auth.service';

/** The API prefix this application talks to, for telling our URLs from everyone else's. */
function apiBase(): string {
  return '/api';
}

/**
 * Attaches the bearer token to every request and signs the user out when the server
 * rejects it.
 *
 * Handled centrally so no repository has to remember the header, and so an expired or
 * revoked token cannot leave the UI in a half-signed-in state.
 */
export const authInterceptor: HttpInterceptorFn = (request, next) => {
  const auth = inject(AuthService);
  const router = inject(Router);

  const token = auth.token();
  // Only this API gets the token. Image bytes go straight to object storage on AWS, and an
  // `Authorization` header there would be at best ignored and at worst a signature mismatch;
  // a rejection from S3 is not this session being over either.
  const external = /^https?:\/\//i.test(request.url) && !request.url.startsWith(apiBase());
  const authorised =
    token && !external
      ? request.clone({ setHeaders: { Authorization: `Bearer ${token}` } })
      : request;

  return next(authorised).pipe(
    catchError((error: HttpErrorResponse) => {
      if (error.status === 401 && !external) {
        auth.clear();
        router.navigate(['/login']);
      }
      return throwError(() => error);
    }),
  );
};
