import { Component, EventEmitter, Output, inject } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatToolbarModule } from '@angular/material/toolbar';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe } from '@jsverse/transloco';

import { RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth/auth.service';
import { CapabilitiesService } from '../../core/capabilities/capabilities.service';
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
  /** Whether the navigation should be shown or put away; the layout owns the answer. */
  @Output() toggleNavigation = new EventEmitter<void>();

  public auth = inject(AuthService);

  /**
   * What this deployment calls itself, or null where it never said: the brand then reads as the
   * product's name alone, which is what every deployment showed before the name existed.
   */
  public siteName = inject(CapabilitiesService).siteName;

  logout() {
    this.auth.logout();
  }
}
