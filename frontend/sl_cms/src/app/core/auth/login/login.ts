import { Component, inject, signal } from '@angular/core';
import { TranslocoPipe } from '@jsverse/transloco';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCardModule } from '@angular/material/card';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';

import { CapabilitiesService } from '../../capabilities/capabilities.service';
import { LanguageSwitcher } from '../../../shared/language-switcher/language-switcher';
import { AuthService } from '../auth.service';

@Component({
  selector: 'app-login',
  imports: [
    FormsModule,
    LanguageSwitcher,
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
  /** The message key, not the message: the wording lives in the catalogs. */
  public error = signal('');
  /** What the message needs filled in, if anything (`{seconds}`). */
  public errorParams = signal<Record<string, unknown>>({});
  public busy = signal(false);

  constructor() {
    this.capabilities.load();
  }

  submit() {
    if (this.busy()) {
      return;
    }
    this.error.set('');
    this.errorParams.set({});
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
          this.error.set('auth.tooManyAttempts');
          this.errorParams.set({
            seconds: Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : 0,
          });
          return;
        }
        // The API answers with a plain-text English message; it is shown as it came, which is
        // what a client that does not know the reason can honestly do.
        this.error.set(
          typeof response?.error === 'string' && response.error
            ? response.error
            : 'auth.signInFailed',
        );
      },
    });
  }
}
