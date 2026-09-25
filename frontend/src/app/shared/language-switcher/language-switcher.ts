import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';

import { SUPPORTED_LANGUAGES, Language } from '../../core/i18n/language';
import { LanguageService, languageLabel } from '../../core/i18n/language.service';

/**
 * The manual switch.
 *
 * The interface follows the browser until someone says otherwise, and this is how they say it —
 * including on the sign-in screen, because a person who cannot read the language the browser
 * guessed is exactly the person who cannot find a switch hidden behind a sign-in.
 */
@Component({
  selector: 'app-language-switcher',
  imports: [MatButtonModule],
  template: `
    @for (language of languages; track language) {
      <button
        matButton
        type="button"
        [class.current]="language === current()"
        [attr.aria-pressed]="language === current()"
        (click)="use(language)"
      >
        {{ label(language) }}
      </button>
    }
  `,
  styles: `
    :host {
      display: flex;
      gap: 0.25rem;
      justify-content: flex-end;
    }
    .current {
      text-decoration: underline;
      font-weight: 600;
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LanguageSwitcher {
  private languages_ = inject(LanguageService);

  protected readonly languages = SUPPORTED_LANGUAGES;
  protected readonly current = this.languages_.language;

  protected use(language: Language): void {
    this.languages_.use(language);
  }

  protected label(language: Language): string {
    return languageLabel(language);
  }
}
