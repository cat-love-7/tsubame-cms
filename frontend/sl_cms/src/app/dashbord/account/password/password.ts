import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { TranslocoPipe } from '@jsverse/transloco';

import { AuthService } from 'app/core/auth/auth.service';
import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { UsersService } from 'app/services/auth/users.service';

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
  ],
  templateUrl: './password.html',
  styleUrl: './password.scss',
})
export class Password {
  private users = inject(UsersService);
  private auth = inject(AuthService);

  public username = this.auth.user()?.username ?? '';
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
