import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { RouterOutlet } from '@angular/router';

import { CapabilitiesService } from './core/capabilities/capabilities.service';
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
    // Asked once per page load, here, so no screen has to be the first to need the answer: where
    // to sign in, and how large an image this deployment takes (the upload screens refuse a file
    // over it before sending anything).
    inject(CapabilitiesService).load();
  }
}
