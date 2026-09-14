import { EnvironmentProviders } from '@angular/core';
import { provideTransloco } from '@jsverse/transloco';

import { FALLBACK_LANGUAGE, SUPPORTED_LANGUAGES } from './language';
import { BundledTranslocoLoader } from './transloco.loader';

/**
 * Transloco for a test, fixed to English.
 *
 * Every spec that renders a screen with translated text needs this, and every one of them wants
 * the same thing: the real catalogs (so a key that does not exist is a failure) in a known
 * language (so a check does not depend on the machine it runs on). A test about switching calls
 * `use('ja')` itself.
 */
export function provideTestTransloco(): EnvironmentProviders[] {
  return provideTransloco({
    config: {
      availableLangs: [...SUPPORTED_LANGUAGES],
      defaultLang: FALLBACK_LANGUAGE,
      fallbackLang: FALLBACK_LANGUAGE,
      reRenderOnLangChange: true,
      prodMode: false,
    },
    loader: BundledTranslocoLoader,
  });
}
