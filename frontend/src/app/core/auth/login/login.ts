import { Component, inject, signal } from '@angular/core';
import { TranslocoPipe } from '@jsverse/transloco';
import { FormsModule } from '@angular/forms';
import { HttpErrorResponse } from '@angular/common/http';
import { Router } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCardModule } from '@angular/material/card';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';

import { CapabilitiesService } from '../../capabilities/capabilities.service';
import { Message, MessagePipe, apiMessage, t } from '../../i18n/message';
import { LanguageSwitcher } from '../../../shared/language-switcher/language-switcher';
import { AuthService } from '../auth.service';
import { beginHostedLogin } from '../hosted-login';

@Component({
  selector: 'app-login',
  imports: [
    FormsModule,
    LanguageSwitcher,
    MessagePipe,
    MatButtonModule,
    MatCardModule,
    MatFormFieldModule,
    MatInputModule,
    TranslocoPipe,
  ],
  templateUrl: './login.html',
  styleUrl: './login.scss',
})
export class Login {
  private auth = inject(AuthService);
  private router = inject(Router);
  private capabilities = inject(CapabilitiesService);

  /**
   * Whether this deployment signs users in itself. Where it does not, the form would be a
   * password box that always answers 501: the sign-in belongs to the identity provider, and
   * all the CMS can usefully say is so.
   */
  public passwordLogin = this.capabilities.passwordLogin;
  public loginUrl = this.capabilities.loginUrl;
  /**
   * What this deployment calls itself, or null where it never said: the card then names the
   * product instead, which is what every deployment showed before the name existed.
   */
  public siteName = this.capabilities.siteName;

  public username = '';
  public password = '';
  /** The failure to show, as a key or as the server's own words: the wording lives in the catalogs. */
  public error = signal<Message | null>(null);
  public busy = signal(false);

  constructor() {
    this.capabilities.load();
    // Someone who is already signed in has no business on this screen: a bookmark, a back button
    // or a typed address would otherwise show a sign-in form that cannot improve on the session
    // they have. The token is read synchronously from storage, so this is not a race with a load.
    if (this.auth.user() !== null) {
      void this.router.navigate(['/']);
    }
  }

  /**
   * Go to the identity provider's sign-in page, with everything only this browser knows.
   *
   * The address the deployment advertises names the client and the scope; the PKCE challenge and
   * the `state` are added here (see `hosted-login.ts`), and the provider sends the browser back to
   * `/auth/callback`, where the code becomes a session.
   */
  async signInAtProvider() {
    if (this.busy()) {
      return;
    }
    this.error.set(null);
    this.busy.set(true);
    try {
      const address = await this.providerSignInUrl();
      if (address === null) {
        this.busy.set(false);
        return;
      }
      window.location.assign(address);
    } catch {
      // Only a browser that cannot do the cryptography gets here.
      this.busy.set(false);
      this.error.set({ key: 'auth.signInNotStarted' });
    }
  }

  /**
   * The address to visit, with the PKCE challenge and the state this browser will need back.
   *
   * Kept separate from the navigation so the interesting half can be tested: a browser refuses to
   * let a page watch its own `location.assign`.
   */
  async providerSignInUrl(): Promise<string | null> {
    const loginUrl = this.loginUrl();
    return loginUrl ? beginHostedLogin(loginUrl) : null;
  }

  submit() {
    if (this.busy()) {
      return;
    }
    this.error.set(null);
    this.busy.set(true);

    this.auth.login(this.username.trim(), this.password).subscribe({
      next: () => {
        this.busy.set(false);
        void this.router.navigate(['/']);
      },
      error: (response: HttpErrorResponse) => {
        this.busy.set(false);
        if (response?.status === 429) {
          // Too many failures: `Retry-After` says how long the wait is, so the message can be
          // about waiting rather than about the password.
          const seconds = Number(response.headers?.get('Retry-After'));
          this.error.set(
            t('auth.tooManyAttempts', {
              seconds: Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : 0,
            }),
          );
          return;
        }
        // Otherwise the API's own answer: the wording of a code this client knows, or the
        // English message as it came, which is what a client that does not know the reason can
        // honestly show. Nothing at all means the request never reached the server.
        const body: unknown = response?.error;
        const answered = (typeof body === 'string' && body !== '') || typeof body === 'object';
        this.error.set(answered ? apiMessage(response) : t('auth.signInFailed'));
      },
    });
  }
}
