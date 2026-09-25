import { inject } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';

import { AuthService } from './auth.service';

/**
 * Keeps the screens only an administrator may open out of everyone else's reach.
 *
 * The navigation does not offer them either, but a link is not a rule: the address bar, a
 * bookmark and the browser's own history are all ways in, and a reader who lands on a schema
 * editor they cannot save is being told "no" by a refusal they did not ask for. The server is
 * still what enforces this (it refuses the write), so this only decides what is worth showing.
 *
 * Not to be confused with the *reading* of a schema, which every signed-in account may do: the
 * content editor is drawn from it. That is the API, not these screens.
 */
export const adminGuard: CanActivateFn = () => {
  const auth = inject(AuthService);
  const router = inject(Router);

  return auth.isAdmin() ? true : router.createUrlTree(['/']);
};
