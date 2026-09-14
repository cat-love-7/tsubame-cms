import { TestBed } from '@angular/core/testing';

import { DateTimeFormat, formatDateTime } from './date-format';
import { LanguageService } from './language.service';

describe('formatDateTime', () => {
  it('writes the date and time the way the language does', () => {
    const value = new Date('2024-05-06T07:08:09Z');

    expect(formatDateTime(value, 'en')).toBe(
      new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeStyle: 'short' }).format(value),
    );
    expect(formatDateTime(value, 'ja')).toBe(
      new Intl.DateTimeFormat('ja', { dateStyle: 'medium', timeStyle: 'short' }).format(value),
    );
    // Not the same string, or there would be nothing to switch for.
    expect(formatDateTime(value, 'en')).not.toBe(formatDateTime(value, 'ja'));
  });

  it('takes what the API sends, and shows a dash for what cannot be read', () => {
    expect(formatDateTime('2024-05-06T07:08:09Z', 'en')).toBe(
      formatDateTime(new Date('2024-05-06T07:08:09Z'), 'en'),
    );
    expect(formatDateTime(null, 'en')).toBe('—');
    expect(formatDateTime('', 'en')).toBe('—');
    expect(formatDateTime('not a date', 'en')).toBe('—');
  });

  it('follows the chosen language when a screen asks it to', () => {
    const language = TestBed.inject(LanguageService);
    const formatter = TestBed.inject(DateTimeFormat);
    const value = '2024-05-06T07:08:09Z';

    expect(formatter.format(value)).toBe(formatDateTime(value, 'en'));

    language.use('ja');
    expect(formatter.format(value)).toBe(formatDateTime(value, 'ja'));
  });
});
