import { FieldSchema } from 'app/models/schema/fields';

/** Height of one layout row unit. `height` in a schema is a multiple of this. */
export const FIELD_ROW_UNIT = '72px';

/**
 * Inline style for a field's cell in the shared 12-column layout grid.
 *
 * `width` is a colspan (1-12) and maps directly onto the grid columns. `height` is a
 * *minimum* height in row units: the real height of a form widget is decided by the
 * widget itself (a six-row textarea is taller than a checkbox), so treating `height` as
 * a fixed rowspan would fight the content.
 *
 * Both the schema editor and the content editor use this, so what the schema editor
 * shows is what the content editor renders.
 */
export function fieldCellStyle(field: FieldSchema): Record<string, string> {
  return {
    'grid-column': `span ${field.width}`,
    // The row unit is a CSS variable (set by `.field-grid`) so the density can be themed
    // without touching this code; the fallback keeps the value meaningful if it is not.
    'min-height': `calc(var(--field-row-unit, ${FIELD_ROW_UNIT}) * ${field.height})`,
  };
}

/**
 * Convenience widths, expressed in 12-column grid units.
 *
 * Typing a raw column count is not intuitive; these cover the common fractions and the
 * numeric input remains available for anything else.
 */
export const FieldWidthPresets: { label: string; width: number }[] = [
  { label: '1/4', width: 3 },
  { label: '1/3', width: 4 },
  { label: '1/2', width: 6 },
  { label: '2/3', width: 8 },
  { label: '3/4', width: 9 },
  { label: 'Full', width: 12 },
];
