import { Component, inject, signal, ChangeDetectionStrategy } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCardModule } from '@angular/material/card';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';

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
  changeDetection: ChangeDetectionStrategy.Eager,
  styleUrl: './login.scss',
})
export class Login {
  private auth = inject(AuthService);
  private router = inject(Router);

  public email = '';
  public password = '';
  public error = signal('');
  public busy = signal(false);

  submit() {
    if (this.busy()) {
      return;
    }
    this.error.set('');
    this.busy.set(true);

    this.auth.login(this.email.trim(), this.password).subscribe({
      next: () => {
        this.busy.set(false);
        this.router.navigate(['/']);
      },
      error: (response) => {
        this.busy.set(false);
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
