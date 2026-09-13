import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCardModule } from '@angular/material/card';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';

import { AuthService } from '../auth.service';
import { errorMessage as message } from 'app/core/http-error';

/**
 * Set a new password with a link an administrator issued.
 *
 * Public, like the sign-in screen: someone who cannot sign in is exactly who this is for. The
 * token in the URL is the whole credential, and it stops working the moment this succeeds.
 */
@Component({
  selector: 'app-password-reset',
  imports: [FormsModule, MatButtonModule, MatCardModule, MatFormFieldModule, MatInputModule, RouterLink],
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
  public error = signal('');
  public busy = signal(false);

  save() {
    if (this.busy()) {
      return;
    }
    if (this.next.length < 8) {
      this.error.set('新しいパスワードは 8 文字以上にしてください');
      return;
    }
    if (this.next !== this.repeated) {
      this.error.set('新しいパスワードが一致しません');
      return;
    }

    this.error.set('');
    this.busy.set(true);
    this.auth.completePasswordReset(this.token, this.next).subscribe({
      next: () => {
        this.busy.set(false);
        // Signed in with the new password: there is no reason to ask for it again.
        this.router.navigate(['/']);
      },
      error: (e) => {
        this.busy.set(false);
        this.error.set(
          typeof e?.error === 'string' && e.error
            ? e.error
            : `Could not set the password: ${message(e)}`,
        );
      },
    });
  }
}
