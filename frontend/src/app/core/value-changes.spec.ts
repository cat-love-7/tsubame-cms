import { describe, expect, it } from 'vitest';

import { fingerprint } from './value-changes';

describe('fingerprint', () => {
  it('is the same for the same values, whatever order they were built in', () => {
    expect(fingerprint({ title: 'a', tags: ['x', 'y'] })).toBe(
      fingerprint({ tags: ['x', 'y'], title: 'a' }),
    );
    // Order inside an array is the value, so that is not reordered.
    expect(fingerprint(['x', 'y'])).not.toBe(fingerprint(['y', 'x']));
  });

  it('changes when a value changes, at any depth', () => {
    const before = { title: 'a', parts: [{ id: 'p1', values: { label: 'one' } }] };
    expect(fingerprint({ ...before, title: 'b' })).not.toBe(fingerprint(before));
    expect(fingerprint({ ...before, parts: [{ id: 'p1', values: { label: 'two' } }] })).not.toBe(
      fingerprint(before),
    );
    // An empty form and a form with a field filled in differ.
    expect(fingerprint({ title: '' })).not.toBe(fingerprint({}));
  });
});
