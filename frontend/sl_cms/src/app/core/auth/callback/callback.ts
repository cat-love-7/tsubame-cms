import { Component, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCardModule } from '@angular/material/card';
import { TranslocoPipe } from '@jsverse/transloco';

import { AuthService } from '../auth.service';
import { Message, MessagePipe, apiMessage } from 'app/core/i18n/message';
import { callbackUrl, forgetSignIn, pendingSignIn } from '../hosted-login';

/**
 * Where the identity provider sends the browser back to.
 *
 * Public, like the sign-in screen: the code in the address is the whole credential. Two things are
 * checked before it is used - that a sign-in is actually in flight in this tab, and that the
 * `state` is the one this tab started with - and the code is exchanged by the server, which is
 * what has the provider's token endpoint.
 */
@Component({
  selector: 'app-auth-callback',
  imports: [MatButtonModule, MatCardModule, MessagePipe, RouterLink, TranslocoPipe],
  templateUrl: './callback.html',
  styleUrl: './callback.scss',
})
export class AuthCallback implements OnInit {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private auth = inject(AuthService);

  public error = signal<Message | null>(null);
  public busy = signal(true);

  ngOnInit() {
    const code = this.route.snapshot.queryParamMap.get('code');
    const state = this.route.snapshot.queryParamMap.get('state');
    const pending = pendingSignIn();

    if (!code) {
      // The provider came back without a code: a cancelled sign-in says `error=access_denied`.
      this.busy.set(false);
      this.error.set({ key: 'auth.signInCancelled' });
      return;
    }
    if (pending === null || state !== pending.state) {
      // Nothing started here, or the answer belongs to a sign-in this tab did not begin. The code
      // is not used either way.
      this.busy.set(false);
      this.error.set({ key: 'auth.signInNotOurs' });
      return;
    }

    this.auth.completeHostedLogin(code, pending.verifier, callbackUrl()).subscribe({
      next: () => {
        // The code is single-use; the verifier goes with it.
        forgetSignIn();
        this.busy.set(false);
        void this.router.navigate(['/']);
      },
      error: (e) => {
        forgetSignIn();
        this.busy.set(false);
        this.error.set(apiMessage(e));
      },
    });
  }
}
