import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { RouterOutlet } from '@angular/router';

import { LanguageService } from './core/i18n/language.service';

@Component({
  selector: 'app-root',
  imports: [RouterOutlet],
  templateUrl: './app.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  protected readonly title = signal('sl_cms');

  constructor() {
    // Before anything renders: the catalogs have to know which language they are in, and the
    // choice is the user's or the browser's (doc/i18n.md).
    inject(LanguageService).initialize();
  }
}
