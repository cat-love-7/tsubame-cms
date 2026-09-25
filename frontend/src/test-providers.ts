import { provideTestTransloco } from './app/core/i18n/testing';

/**
 * Providers every test environment gets, wired in `angular.json` (`providersFile`).
 *
 * Almost every screen renders translated text, so almost every spec needs the catalogs; asking
 * each one to remember the provider would mean 35 copies of the same three lines and a
 * confusing failure when one was forgotten. The tests fixed to English is exactly step 5 of
 * `docs/i18n.md`.
 */
export default [...provideTestTransloco()];
