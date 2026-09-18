import { describe, expect, it } from 'vitest';

import { FieldSchema } from 'app/models/schema/fields';
import { FieldWidthPresets, fieldCellStyle, resizeHeight, resizeWidth } from './field-layout';

function field(width: number, height: number): FieldSchema {
  return { name: 'f', field_type: 'Number', required: false, width, height };
}

describe('fieldCellStyle', () => {
  it('maps width onto grid columns', () => {
    expect(fieldCellStyle(field(6, 1))['grid-column']).toBe('span 6');
    expect(fieldCellStyle(field(12, 1))['grid-column']).toBe('span 12');
  });

  it('treats height as a minimum, not a fixed rowspan', () => {
    expect(fieldCellStyle(field(12, 2))['min-height']).toBe(
      'calc(var(--field-row-unit, 72px) * 2)',
    );
  });
});

describe('FieldWidthPresets', () => {
  it('expresses the presets in 12-column units', () => {
    expect(FieldWidthPresets.find((p) => p.label === '1/2')?.width).toBe(6);
    expect(FieldWidthPresets.find((p) => p.label === 'Full')?.width).toBe(12);
    // Every preset must be a valid width for `validate_schema`.
    for (const preset of FieldWidthPresets) {
      expect(preset.width).toBeGreaterThanOrEqual(1);
      expect(preset.width).toBeLessThanOrEqual(12);
    }
  });
});

describe('resizeWidth', () => {
  const COLUMN = 100;

  it('snaps the drag distance to whole columns', () => {
    expect(resizeWidth(4, 200, COLUMN)).toBe(6);
    expect(resizeWidth(4, -100, COLUMN)).toBe(3);
    // Just past half a column still rounds to the nearest one.
    expect(resizeWidth(4, 149, COLUMN)).toBe(5);
  });

  it('clamps to the grid', () => {
    expect(resizeWidth(11, 500, COLUMN)).toBe(12);
    expect(resizeWidth(2, -500, COLUMN)).toBe(1);
  });

  it('leaves the width alone when the grid cannot be measured', () => {
    // Guards against dividing by zero on an unlaid-out grid (e.g. in tests).
    expect(resizeWidth(7, 200, 0)).toBe(7);
    expect(resizeWidth(99, 200, 0)).toBe(12);
  });
});

describe('resizeHeight', () => {
  const ROW = 72;

  it('snaps the drag distance to whole row units', () => {
    expect(resizeHeight(1, 144, ROW)).toBe(3);
    expect(resizeHeight(3, -72, ROW)).toBe(2);
  });

  it('never drops below one row', () => {
    expect(resizeHeight(1, -500, ROW)).toBe(1);
  });

  it('leaves the height alone when the row unit is unknown', () => {
    expect(resizeHeight(2, 144, 0)).toBe(2);
  });
});
