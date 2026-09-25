/**
 * Which language the interface is in.
 *
 * The order is the decision recorded in `doc/i18n.md`: what the user chose, then what the
 * browser asks for, then English. Regional values are resolved by dropping the region —
 * `ja-JP` is `ja` — so adding a regional catalogue later does not change this function, only the
 * list of what is available.
 */
export const SUPPORTED_LANGUAGES = ['en', 'ja'] as const;

export type Language = (typeof SUPPORTED_LANGUAGES)[number];

/** Where an unsupported preference lands. */
export const FALLBACK_LANGUAGE: Language = 'en';

/** Where the choice is kept. */
export const LANGUAGE_STORAGE_KEY = 'sl_cms.language';

/** The base language of a tag: `ja-JP` → `ja`, `EN-gb` → `en`. */
export function baseLanguage(tag: string): string {
  return tag.trim().toLowerCase().split('-')[0];
}

function isSupported(candidate: string): candidate is Language {
  return (SUPPORTED_LANGUAGES as readonly string[]).includes(candidate);
}

/**
 * The language to use, given a saved choice and the browser's preferences in order.
 *
 * A saved choice that is no longer supported falls through to the browser rather than being
 * honoured: a catalogue that was removed should not leave the interface in it.
 */
export function resolveLanguage(
  saved: string | null | undefined,
  preferred: readonly string[],
): Language {
  if (saved) {
    const candidate = baseLanguage(saved);
    if (isSupported(candidate)) {
      return candidate;
    }
  }
  for (const tag of preferred) {
    const candidate = baseLanguage(tag);
    if (isSupported(candidate)) {
      return candidate;
    }
  }
  return FALLBACK_LANGUAGE;
}
