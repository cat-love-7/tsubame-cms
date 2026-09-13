import { describe, expect, it } from 'vitest';

import { FieldSchema } from 'app/models/schema/fields';
import { FieldWidthPresets, fieldCellStyle } from './field-layout';

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
