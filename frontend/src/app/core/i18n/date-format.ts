import { Injectable, inject } from '@angular/core';

import { LanguageService } from './language.service';

/**
 * How a date and time are written in `language`.
 *
 * `Intl` (through the browser's own locale data) rather than a fixed pattern, so 2024-05-06 is
 * "May 6, 2024, 7:08 AM" in English and "2024/05/06 7:08" in Japanese. A value that is missing or
 * unparseable renders as a dash, which is what the tables show for "never".
 */
export function formatDateTime(
  value: string | number | Date | null | undefined,
  language: string,
): string {
  if (value === null || value === undefined || value === '') {
    return '—';
  }
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) {
    return '—';
  }
  return new Intl.DateTimeFormat(language, { dateStyle: 'medium', timeStyle: 'short' }).format(
    date,
  );
}

/**
 * The same, for a template.
 *
 * Reading the language signal while the view is being checked is what makes a rendered date
 * follow a language change instead of freezing in the language it was first drawn in.
 */
@Injectable({ providedIn: 'root' })
export class DateTimeFormat {
  private language = inject(LanguageService).language;

  format(value: string | number | Date | null | undefined): string {
    return formatDateTime(value, this.language());
  }
}
