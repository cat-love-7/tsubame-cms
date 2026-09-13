import { Component, inject } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatToolbarModule } from '@angular/material/toolbar';

import { RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth/auth.service';

@Component({
  selector: 'app-header',
  templateUrl: './header.html',
  styleUrl: './header.scss',
  imports: [
    MatButtonModule,
    MatIconModule,
    MatToolbarModule,
    RouterLink
  ],
})
export class Header {
  public auth = inject(AuthService);

  logout() {
    this.auth.logout();
  }
}
