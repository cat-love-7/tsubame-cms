import { describe, expect, it } from 'vitest';

import { reconcileArrayItemTypes } from './fields';

describe('reconcileArrayItemTypes', () => {
  it('leaves selections without a conflict untouched', () => {
    expect(reconcileArrayItemTypes([], ['Number', 'Boolean'])).toEqual(['Number', 'Boolean']);
  });

  it('allows an image-only array', () => {
    expect(reconcileArrayItemTypes([], ['Image'])).toEqual(['Image']);
  });

  it('drops Number when Image is added', () => {
    expect(reconcileArrayItemTypes(['Number'], ['Number', 'Image'])).toEqual(['Image']);
  });

  it('drops Image when Number is added', () => {
    expect(reconcileArrayItemTypes(['Image'], ['Image', 'Number'])).toEqual(['Number']);
  });

  it('keeps other item types alongside the winner', () => {
    expect(reconcileArrayItemTypes(['Number', 'Boolean'], ['Number', 'Boolean', 'Image'])).toEqual([
      'Boolean',
      'Image',
    ]);
  });
});
