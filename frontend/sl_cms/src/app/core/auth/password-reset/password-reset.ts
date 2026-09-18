import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCardModule } from '@angular/material/card';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { TranslocoPipe } from '@jsverse/transloco';

import { AuthService } from '../auth.service';
import { Message, MessagePipe, apiMessage, t } from 'app/core/i18n/message';

/**
 * Set a new password with a link an administrator issued.
 *
 * Public, like the sign-in screen: someone who cannot sign in is exactly who this is for. The
 * token in the URL is the whole credential, and it stops working the moment this succeeds.
 */
@Component({
  selector: 'app-password-reset',
  imports: [
    FormsModule,
    MatButtonModule,
    MatCardModule,
    MatFormFieldModule,
    MatInputModule,
    MessagePipe,
    RouterLink,
    TranslocoPipe,
  ],
  templateUrl: './password-reset.html',
  styleUrl: './password-reset.scss',
})
export class PasswordReset {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private auth = inject(AuthService);

  private token = this.route.snapshot.queryParamMap.get('token') ?? '';
  public next = '';
  public repeated = '';
  public error = signal<Message | null>(null);
  public busy = signal(false);

  save() {
    if (this.busy()) {
      return;
    }
    if (this.next.length < 8) {
      this.error.set(t('auth.passwordTooShort'));
      return;
    }
    if (this.next !== this.repeated) {
      this.error.set(t('auth.passwordMismatch'));
      return;
    }

    this.error.set(null);
    this.busy.set(true);
    this.auth.completePasswordReset(this.token, this.next).subscribe({
      next: () => {
        this.busy.set(false);
        // Signed in with the new password: there is no reason to ask for it again.
        void this.router.navigate(['/']);
      },
      error: (e) => {
        this.busy.set(false);
        // The link itself was refused (used already, past its expiry): the server's answer is
        // the whole story, and it is the only thing this screen can honestly say.
        this.error.set(apiMessage(e));
      },
    });
  }
}
