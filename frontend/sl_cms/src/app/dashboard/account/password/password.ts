import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { TranslocoPipe } from '@jsverse/transloco';

import { AuthService } from 'app/core/auth/auth.service';
import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { UsersService } from 'app/services/auth/users.service';
import { NoticeToast } from 'app/shared/notice-toast/notice-toast';

/**
 * Change your own password.
 *
 * Open to every signed-in account, including a read-only one: the current password is
 * required, so a stolen session is not enough to lock the owner out.
 */
@Component({
  selector: 'app-password',
  imports: [
    FormsModule,
    MatButtonModule,
    MatFormFieldModule,
    MatInputModule,
    MessagePipe,
    TranslocoPipe,
    NoticeToast,
  ],
  templateUrl: './password.html',
  styleUrl: './password.scss',
})
export class Password {
  private users = inject(UsersService);
  private auth = inject(AuthService);
  private capabilities = inject(CapabilitiesService);

  public username = this.auth.user()?.username ?? '';
  /**
   * Whether this deployment checks the password at all.
   *
   * Where it does not, the form below would only ever answer 501: the credential belongs to an
   * identity provider, and an administrator resets it from the account screen. Saying that is
   * more use than a form that cannot work.
   */
  public passwordLogin = this.capabilities.passwordLogin;
  public current = '';
  public next = '';
  public repeated = '';
  /** The failure or the outcome, as keys, so the screen follows a language change. */
  public error = signal<Message | null>(null);
  public status = signal<Message | null>(null);

  save() {
    if (this.next.length < 8) {
      this.error.set(t('auth.passwordTooShort'));
      return;
    }
    if (this.next !== this.repeated) {
      this.error.set(t('auth.passwordMismatch'));
      return;
    }

    this.error.set(null);
    this.status.set(null);
    this.users.changeOwnPassword(this.current, this.next).subscribe({
      next: (changed) => {
        // Every session from before the change is gone, this one included; the server hands
        // back a replacement token so the screen the user is on keeps working.
        this.auth.replaceToken(changed.token);
        this.current = '';
        this.next = '';
        this.repeated = '';
        this.status.set(t('auth.passwordChangedOwn'));
      },
      error: (e) => this.error.set(failure('auth.changePasswordFailed', e)),
    });
  }
}
