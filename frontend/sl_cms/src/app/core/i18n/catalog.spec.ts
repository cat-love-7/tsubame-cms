import en from '../../../assets/i18n/en.json';
import errorCodes from '../../../assets/error-codes.json';
import ja from '../../../assets/i18n/ja.json';

/**
 * The translations are data, and data drifts: a key added to one language and forgotten in the
 * other shows up as a half-empty screen in whichever language nobody tested. These tests are
 * what stops that, rather than a review that has to remember.
 */
const LANGUAGES = ['en', 'ja'] as const;

/** Every leaf key, dotted. */
function keys(tree: Record<string, unknown>, prefix = ''): string[] {
  return Object.entries(tree).flatMap(([key, value]) =>
    value !== null && typeof value === 'object'
      ? keys(value as Record<string, unknown>, `${prefix}${key}.`)
      : [`${prefix}${key}`],
  );
}

describe('translation catalogs', () => {
  // Imported rather than read from disk: the catalogs are part of the build, so a file that
  // does not parse fails the build itself, and no Node file APIs are needed here.
  const catalogs: Record<string, Record<string, unknown>> = {
    en: en as Record<string, unknown>,
    ja: ja as Record<string, unknown>,
  };

  it('hold exactly the same keys', () => {
    const [english, japanese] = LANGUAGES.map((language) => new Set(keys(catalogs[language])));
    const missing = [...english].filter((key) => !japanese.has(key));
    const extra = [...japanese].filter((key) => !english.has(key));

    expect(missing, 'keys the Japanese catalog is missing').toEqual([]);
    expect(extra, 'keys only the Japanese catalog has').toEqual([]);
  });

  it('leave no string empty, because an empty label is worse than a missing one', () => {
    for (const language of LANGUAGES) {
      for (const key of keys(catalogs[language])) {
        const value = key
          .split('.')
          .reduce<unknown>((node, part) => (node as Record<string, unknown>)[part], catalogs[language]);
        expect(typeof value, `${language}:${key}`).toBe('string');
        expect((value as string).trim(), `${language}:${key}`).not.toBe('');
      }
    }
  });

  it('translate every error code the API can answer with', () => {
    // The codes are the contract the server publishes (a Rust test keeps its copy in step);
    // this is the other side of it, so a code the UI cannot word is caught here rather than
    // showing English to someone who chose Japanese.
    const codes = [...errorCodes.situational, ...errorCodes.status];
    for (const language of LANGUAGES) {
      const errors = (catalogs[language]['errors'] ?? {}) as Record<string, string>;
      for (const code of codes) {
        expect(errors[code], `${language}: errors.${code}`).toBeTruthy();
      }
    }
  });

  it('share the placeholders of every message, so a translation cannot drop one', () => {
    const placeholders = (text: string) => (text.match(/\{[a-z]+\}/g) ?? []).sort();
    for (const key of keys(catalogs['en'])) {
      const value = key
        .split('.')
        .reduce<unknown>((node, part) => (node as Record<string, unknown>)[part], catalogs['en']) as string;
      const translation = key
        .split('.')
        .reduce<unknown>((node, part) => (node as Record<string, unknown>)[part], catalogs['ja']) as string;
      expect(placeholders(translation), `ja:${key}`).toEqual(placeholders(value));
    }
  });
});
