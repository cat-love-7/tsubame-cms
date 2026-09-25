import { describe, expect, it } from 'vitest';

import { isUsableSlug, normaliseSlug } from './slug';

/**
 * The same table the Rust implementation is tested against (`models/slug.rs`), so the two cannot
 * drift into disagreeing about what a slug is: the screen shows one thing and stores another
 * otherwise.
 */
const CASES: [string, string][] = [
  ['Hello World', 'hello-world'],
  ['hello-world', 'hello-world'],
  ['  Hello, World!  ', 'hello-world'],
  ['One -- Two', 'one-two'],
  ['Already-slugged_1', 'already-slugged-1'],
  ['--leading and trailing--', 'leading-and-trailing'],
  ['CamelCaseTitle', 'camelcasetitle'],
  ['Ünïcödé', 'n-c-d'],
  ['café au lait', 'caf-au-lait'],
  ['日本語', ''],
  ['', ''],
  ['-', ''],
  ['2026-09-15 release', '2026-09-15-release'],
];

describe('normaliseSlug', () => {
  it('keeps letters, digits and single hyphens, and nothing else', () => {
    for (const [input, expected] of CASES) {
      expect(normaliseSlug(input), `normalising ${JSON.stringify(input)}`).toBe(expected);
    }
  });

  it('changes nothing when the value is already canonical', () => {
    for (const [, expected] of CASES) {
      expect(normaliseSlug(expected), `re-normalising ${JSON.stringify(expected)}`).toBe(expected);
    }
  });
});

describe('isUsableSlug', () => {
  it('says when nothing a URL could use is left', () => {
    expect(isUsableSlug('日本語')).toBe(false);
    expect(isUsableSlug('   ')).toBe(false);
    expect(isUsableSlug('a')).toBe(true);
    expect(isUsableSlug('日本語 with a title')).toBe(true);
  });
});
