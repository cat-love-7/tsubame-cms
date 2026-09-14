import { resolveLanguage } from './language';

describe('resolveLanguage', () => {
  it('honours what the user chose', () => {
    expect(resolveLanguage('ja', ['en-US'])).toBe('ja');
    expect(resolveLanguage('en', ['ja-JP'])).toBe('en');
  });

  it('falls back to the browser, in its order of preference', () => {
    expect(resolveLanguage(null, ['fr-FR', 'ja-JP', 'en'])).toBe('ja');
    expect(resolveLanguage(null, ['en-GB', 'ja'])).toBe('en');
  });

  it('resolves a regional tag by its base language', () => {
    expect(resolveLanguage(null, ['ja-JP'])).toBe('ja');
    expect(resolveLanguage(null, ['EN-gb'])).toBe('en');
  });

  it('lands on English when nothing matches', () => {
    expect(resolveLanguage(null, ['fr', 'de-DE'])).toBe('en');
    expect(resolveLanguage(null, [])).toBe('en');
  });

  it('ignores a saved language that is no longer available', () => {
    // A catalogue that was removed must not leave the interface in it.
    expect(resolveLanguage('de', ['ja'])).toBe('ja');
  });
});
