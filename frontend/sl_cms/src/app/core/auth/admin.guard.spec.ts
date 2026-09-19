import { TestBed } from '@angular/core/testing';
import { Router, UrlTree, provideRouter } from '@angular/router';

import { adminGuard } from './admin.guard';
import { AuthService } from './auth.service';

/** The guard asks the account, and nothing else. */
function run(isAdmin: boolean): boolean | UrlTree {
  TestBed.configureTestingModule({
    providers: [provideRouter([]), { provide: AuthService, useValue: { isAdmin: () => isAdmin } }],
  });
  return TestBed.runInInjectionContext(
    () => adminGuard({} as never, {} as never) as boolean | UrlTree,
  );
}

describe('adminGuard', () => {
  it('lets an administrator through', () => {
    expect(run(true)).toBe(true);
  });

  // A link is not a rule: the address bar, a bookmark and the browser's history are all ways in,
  // and a screen whose save is refused is a refusal the reader did not ask for.
  it('sends everyone else to the landing screen', () => {
    const refused = run(false);
    expect(refused).toBeInstanceOf(UrlTree);
    expect(String(refused)).toBe('/');
    expect(TestBed.inject(Router)).toBeTruthy();
  });
});
