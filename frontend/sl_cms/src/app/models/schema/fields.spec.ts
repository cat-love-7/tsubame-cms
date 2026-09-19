import { describe, expect, it } from 'vitest';

import {
  FieldDefaults,
  FieldTypeStringPipe,
  isRelationFieldSchema,
  newFieldType,
  numberOrUndefined,
  reconcileArrayItemTypes,
  schemaForSaving,
} from './fields';

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

describe('schemaForSaving', () => {
  /** A field of the shape the schema editor holds while it is being edited. */
  const text = (options: Record<string, unknown>) => ({
    name: 'title',
    field_type: { Text: options },
    required: false,
    width: 12,
    height: 1,
  });

  // The form's inputs hand back strings - and nothing at all once one is cleared - while the
  // server reads `Option<usize>` strictly: a string there is a 422 about the JSON body.
  it('turns the text limits of every field into numbers', () => {
    const saved = schemaForSaving([text({ max_length: '8', min_length: '5' })] as never);

    expect(saved[0].field_type).toEqual({ Text: { max_length: 8, min_length: 5 } });
  });

  it('drops a limit that was cleared rather than sending an empty string', () => {
    const saved = schemaForSaving([text({ max_length: '', min_length: null })] as never);

    expect(saved[0].field_type).toEqual({ Text: { max_length: undefined, min_length: undefined } });
  });

  it('keeps the other fields as they are', () => {
    const number = { name: 'count', field_type: 'Number', required: false, width: 6, height: 1 };
    const saved = schemaForSaving([number] as never);

    expect(saved[0]).toEqual(number);
  });
});

describe('numberOrUndefined', () => {
  it('reads a number out of what the form held', () => {
    expect(numberOrUndefined('12')).toBe(12);
    expect(numberOrUndefined(12.7)).toBe(12);
    expect(numberOrUndefined('')).toBeUndefined();
    expect(numberOrUndefined(null)).toBeUndefined();
    expect(numberOrUndefined('not a number')).toBeUndefined();
    expect(numberOrUndefined(-1)).toBeUndefined();
  });
});

describe('newFieldType', () => {
  // `FieldDefaults` is one object per type and the field editor writes into the type it is given:
  // sharing it would make one text field's maximum length apply to every other one, and to every
  // text field added afterwards.
  it('gives every field its own copy of the default', () => {
    const first = newFieldType('Text') as { Text: { max_length?: number } };
    const second = newFieldType('Text') as { Text: { max_length?: number } };

    expect(first).toEqual(FieldDefaults.Text);
    expect(first).not.toBe(second);
    expect(first.Text).not.toBe(second.Text);

    first.Text.max_length = 20;
    expect(second.Text.max_length).toBeUndefined();
    expect(FieldDefaults.Text.Text.max_length).toBeUndefined();
  });

  it('copies the kinds that hold arrays and objects', () => {
    const enumField = newFieldType('TextEnum') as { TextEnum: string[] };
    enumField.TextEnum.push('published');
    expect(FieldDefaults.TextEnum.TextEnum).toEqual([]);

    const array = newFieldType('Array') as { Array: unknown[] };
    array.Array.push('Number');
    expect((FieldDefaults.Array as { Array: unknown[] }).Array).toEqual([]);

    // A relation's target is an object too, so one field's choice must not become the default.
    const relation = newFieldType('Relation') as {
      Relation: { target: { name: string } };
    };
    relation.Relation.target.name = 'authors';
    expect(
      (FieldDefaults.Relation as { Relation: { target: { name: string } } }).Relation.target.name,
    ).toBe('');
  });
});

describe('relation fields', () => {
  const relation = (options: Record<string, unknown>) => ({
    name: 'author',
    field_type: { Relation: options },
    required: false,
    width: 12,
    height: 1,
  });

  it('is named Relation, like every other type on the wire', () => {
    expect(new FieldTypeStringPipe().transform(newFieldType('Relation'))).toBe('Relation');
    expect(isRelationFieldSchema(newFieldType('Relation'))).toBe(true);
    expect(isRelationFieldSchema('Number')).toBe(false);
  });

  // A blank name is not a label, and the server refuses one: it is left off the wire instead.
  it('drops a blank inverse name', () => {
    const saved = schemaForSaving([
      relation({
        target: { kind: 'collection', name: 'authors' },
        has_many: true,
        inverse_name: '  ',
      }),
    ] as never);

    expect(saved[0].field_type).toEqual({
      Relation: { target: { kind: 'collection', name: 'authors' }, has_many: true },
    });
  });

  it('trims the inverse name it does send', () => {
    const saved = schemaForSaving([
      relation({
        target: { kind: 'collection', name: 'authors' },
        has_many: false,
        inverse_name: ' articles ',
      }),
    ] as never);

    expect(saved[0].field_type).toEqual({
      Relation: {
        target: { kind: 'collection', name: 'authors' },
        has_many: false,
        inverse_name: 'articles',
      },
    });
  });

  // A page is one item, so "several" is not a choice there and the server refuses it.
  it('never asks for several references to a single page', () => {
    const saved = schemaForSaving([
      relation({ target: { kind: 'single_page', name: 'home' }, has_many: true }),
    ] as never);

    expect(saved[0].field_type).toEqual({
      Relation: { target: { kind: 'single_page', name: 'home' }, has_many: false },
    });
  });
});
