import { Injectable } from '@angular/core';
import { Translation, TranslocoLoader } from '@jsverse/transloco';
import { Observable, of } from 'rxjs';

import en from '../../../assets/i18n/en.json';
import ja from '../../../assets/i18n/ja.json';

/**
 * The catalogs, bundled rather than fetched.
 *
 * Two languages of interface text are a few kilobytes; an HTTP request for them would be a
 * round trip before the first screen can be read, and the catalogs are versioned with the code
 * that uses the keys anyway.
 */
@Injectable({ providedIn: 'root' })
export class BundledTranslocoLoader implements TranslocoLoader {
  getTranslation(language: string): Observable<Translation> {
    const catalogs: Record<string, Translation> = {
      en: en as Translation,
      ja: ja as Translation,
    };
    return of(catalogs[language] ?? (en as Translation));
  }
}
