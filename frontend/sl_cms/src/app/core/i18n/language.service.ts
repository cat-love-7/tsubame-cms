import { DOCUMENT, Injectable, inject, signal } from '@angular/core';
import { TranslocoService } from '@jsverse/transloco';

import {
  FALLBACK_LANGUAGE,
  LANGUAGE_STORAGE_KEY,
  Language,
  baseLanguage,
  resolveLanguage,
} from './language';

/**
 * The chosen language, and the switch for it.
 *
 * Transloco does the translating; this decides *which* language, remembers the choice and tells
 * the document (the `lang` attribute is what a screen reader and the browser's own translation
 * feature read).
 */
@Injectable({ providedIn: 'root' })
export class LanguageService {
  private transloco = inject(TranslocoService);
  private document = inject(DOCUMENT);
  private readonly current = signal<Language>(FALLBACK_LANGUAGE);

  readonly language = this.current.asReadonly();

  /** Adopt the saved choice, or the browser's, and load it. */
  initialize(): void {
    const saved = this.read();
    const preferred = this.browserPreferences();
    const language = resolveLanguage(saved, preferred);
    this.current.set(language);
    this.apply(language);
    this.transloco.setActiveLang(language);
    this.transloco.load(language).subscribe();
  }

  /** Use `language` from now on, and remember it. */
  use(language: Language): void {
    if (language === this.current()) {
      return;
    }
    this.current.set(language);
    this.write(language);
    this.apply(language);
    this.transloco.setActiveLang(language);
    this.transloco.load(language).subscribe();
  }

  private apply(language: Language): void {
    this.document.documentElement.lang = language;
  }

  private browserPreferences(): string[] {
    const navigation = this.document.defaultView?.navigator;
    if (!navigation) {
      return [];
    }
    // `languages` is the ordered list; `language` is the one case where a browser only offers
    // one, and it is the same shape.
    const languages = navigation.languages ?? [];
    return languages.length > 0 ? [...languages] : [navigation.language];
  }

  private read(): string | null {
    try {
      return this.document.defaultView?.localStorage.getItem(LANGUAGE_STORAGE_KEY) ?? null;
    } catch {
      // Storage can be refused (private mode, a locked-down browser). The interface then
      // follows the browser, which is the same as having no choice saved.
      return null;
    }
  }

  private write(language: Language): void {
    try {
      this.document.defaultView?.localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
    } catch {
      // As above: the choice lasts for this page rather than the session.
    }
  }
}

/** For the templates that show a language's own name. */
export function languageLabel(language: Language): string {
  return language === 'ja' ? '日本語' : 'English';
}

/** Exported for the switch, which lists what is available. */
export { baseLanguage };
