import { Component, inject } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatToolbarModule } from '@angular/material/toolbar';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe } from '@jsverse/transloco';

import { RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth/auth.service';
import { LanguageSwitcher } from '../../shared/language-switcher/language-switcher';

@Component({
  selector: 'app-header',
  templateUrl: './header.html',
  styleUrl: './header.scss',
  imports: [
    MatTooltipModule,
    LanguageSwitcher,
    MatButtonModule,
    MatIconModule,
    MatToolbarModule,
    RouterLink,
    TranslocoPipe,
  ],
})
export class Header {
  public auth = inject(AuthService);

  logout() {
    this.auth.logout();
  }
}
