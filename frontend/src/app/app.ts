import { ChangeDetectionStrategy, Component, effect, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { Title } from '@angular/platform-browser';
import { RouterOutlet } from '@angular/router';
import { TranslocoService } from '@jsverse/transloco';

import { CapabilitiesService } from './core/capabilities/capabilities.service';
import { LanguageService } from './core/i18n/language.service';

@Component({
  selector: 'app-root',
  imports: [RouterOutlet],
  templateUrl: './app.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  constructor() {
    // Before anything renders: the catalogs have to know which language they are in, and the
    // choice is the user's or the browser's (docs/i18n.md).
    inject(LanguageService).initialize();
    // Asked once per page load, here, so no screen has to be the first to need the answer: where
    // to sign in, what this deployment is the admin screen for, and how large an image it takes
    // (the upload screens refuse a file over it before sending anything).
    const capabilities = inject(CapabilitiesService);
    capabilities.load();

    // The tab says what is being administered, not only what administers it: a reader who has two
    // CMSs open has nothing else telling them apart, and the tab is the part of the screen that is
    // still there while a form is being filled in. `index.html` holds the product's name for the
    // moment before the deployment answers, and for a deployment that never named itself - the
    // product's name is the whole title then, which is what every deployment showed before a
    // deployment could have a name of its own.
    const title = inject(Title);
    const i18n = inject(TranslocoService);
    // Followed as a signal rather than read once: the catalog arrives a moment after the app is
    // built, and `translate` would then have returned the key itself.
    const product = toSignal(i18n.selectTranslate<string>('app.name'), {
      initialValue: 'Tsubame',
    });
    effect(() => {
      const site = capabilities.siteName();
      title.setTitle(site === null ? product() : `${site} — ${product()}`);
    });
  }
}
