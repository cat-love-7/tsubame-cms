/**
 * Comparing what a form holds with what was loaded.
 *
 * The two are plain JSON trees built by the same code, so the cheap comparison is a rendering of
 * each: keys are sorted on the way, because a value rebuilt by a widget may list the same entries
 * in a different order and a form that only changed by ordering has not changed.
 */
export function fingerprint(value: unknown): string {
  return JSON.stringify(sorted(value));
}

function sorted(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sorted);
  }
  if (value !== null && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
      .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
      .map(([key, entry]) => [key, sorted(entry)]);
    return Object.fromEntries(entries);
  }
  return value;
}
