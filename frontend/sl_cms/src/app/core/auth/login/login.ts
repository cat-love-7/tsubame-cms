import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCardModule } from '@angular/material/card';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';

import { CapabilitiesService } from '../../capabilities/capabilities.service';
import { AuthService } from '../auth.service';

@Component({
  selector: 'app-login',
  imports: [
    FormsModule,
    MatButtonModule,
    MatCardModule,
    MatFormFieldModule,
    MatInputModule,
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
  public error = signal('');
  public busy = signal(false);

  constructor() {
    this.capabilities.load();
  }

  submit() {
    if (this.busy()) {
      return;
    }
    this.error.set('');
    this.busy.set(true);

    this.auth.login(this.username.trim(), this.password).subscribe({
      next: () => {
        this.busy.set(false);
        this.router.navigate(['/']);
      },
      error: (response) => {
        this.busy.set(false);
        if (response?.status === 429) {
          // Too many failures: `Retry-After` says how long the wait is, so the wording can
          // be about waiting rather than about the password.
          const seconds = Number(response.headers?.get('Retry-After'));
          this.error.set(
            'ログインの失敗が続いたため、しばらくロックされています。' +
              (Number.isFinite(seconds) && seconds > 0
                ? `約 ${Math.ceil(seconds / 60)} 分後にやり直してください。`
                : '時間をおいてやり直してください。'),
          );
          return;
        }
        // The API answers with a plain-text message for 401.
        this.error.set(
          typeof response?.error === 'string' && response.error
            ? response.error
            : 'Sign in failed',
        );
      },
    });
  }
}
