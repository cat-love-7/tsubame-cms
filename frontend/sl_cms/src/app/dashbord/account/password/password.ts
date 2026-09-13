import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';

import { AuthService } from 'app/core/auth/auth.service';
import { errorMessage as message } from 'app/core/http-error';
import { UsersService } from 'app/services/auth/users.service';

/**
 * Change your own password.
 *
 * Open to every signed-in account, including a read-only one: the current password is
 * required, so a stolen session is not enough to lock the owner out.
 */
@Component({
  selector: 'app-password',
  imports: [FormsModule, MatButtonModule, MatFormFieldModule, MatInputModule],
  templateUrl: './password.html',
  styleUrl: './password.scss',
})
export class Password {
  private users = inject(UsersService);
  private auth = inject(AuthService);

  public email = this.auth.user()?.email ?? '';
  public current = '';
  public next = '';
  public repeated = '';
  public error = signal('');
  public status = signal('');

  save() {
    if (this.next.length < 8) {
      this.error.set('新しいパスワードは 8 文字以上にしてください');
      return;
    }
    if (this.next !== this.repeated) {
      this.error.set('新しいパスワードが一致しません');
      return;
    }

    this.error.set('');
    this.status.set('');
    this.users.changeOwnPassword(this.current, this.next).subscribe({
      next: () => {
        this.current = '';
        this.next = '';
        this.repeated = '';
        this.status.set('パスワードを変更しました');
      },
      error: (e) => this.error.set(`Could not change the password: ${message(e)}`),
    });
  }
}
