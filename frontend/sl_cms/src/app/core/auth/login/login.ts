import { Component, inject, signal } from '@angular/core';
import { TranslocoPipe } from '@jsverse/transloco';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCardModule } from '@angular/material/card';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';

import { CapabilitiesService } from '../../capabilities/capabilities.service';
import { Message, MessagePipe, apiMessage, t } from '../../i18n/message';
import { LanguageSwitcher } from '../../../shared/language-switcher/language-switcher';
import { AuthService } from '../auth.service';

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

  public username = '';
  public password = '';
  /** The failure to show, as a key or as the server's own words: the wording lives in the catalogs. */
  public error = signal<Message | null>(null);
  public busy = signal(false);

  constructor() {
    this.capabilities.load();
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
        this.router.navigate(['/']);
      },
      error: (response) => {
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
        const body = response?.error;
        const answered = (typeof body === 'string' && body !== '') || typeof body === 'object';
        this.error.set(answered ? apiMessage(response) : t('auth.signInFailed'));
      },
    });
  }
}
