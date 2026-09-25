import { Pipe, PipeTransform } from '@angular/core';

export type TextFieldOptions = {
  max_length?: number;
  min_length?: number;
  /**
   * Whether a `Text` field is written over several lines.
   *
   * A one-line box is right for a title and wrong for a paragraph; the type cannot tell which it
   * is, so the schema says. Markdown is multi-line by nature and ignores it.
   */
  multiline?: boolean;
};

export type TextFieldSchema = {
  Text: TextFieldOptions;
};

/**
 * What a slug field can be told.
 *
 * The character rule and the length cap belong to the type (see `./slug`), so the only thing left
 * is which field an editor is offered to generate the value from.
 */
export type SlugOptions = {
  generate_from?: string;
};

export type SlugFieldSchema = {
  Slug: SlugOptions;
};
export type MarkdownFieldSchema = {
  Markdown: TextFieldOptions;
};

/**
 * The field *type* descriptor for a composite field: just the id of the composite it
 * points at.
 *
 * Not to be confused with the composite's own definition, which is a list of field
 * schemas — see `CompositeFieldDefinition` in `./collection`.
 */
export type CompositeFieldType = {
  CompositeField: {
    id: string;
  };
};

/**
 * An array element is a bare field *type*, matching the Rust
 * `FieldType::Array(Vec<FieldType>)` (it is not a full `FieldSchema`, which would also
 * carry a name and a required flag).
 */
export type ArrayFieldSchema = {
  Array: FieldType[];
};

/**
 * Enum options travel as a JSON array on the wire (`{"TextEnum":["a","b"]}`). A `Set`
 * would serialise to `{}`, so this must stay an array.
 */
export type EnumFieldSchema = {
  TextEnum: string[];
};

/**
 * What a relation points at: the items of another collection, or a single page.
 *
 * `kind` and `name` together are the target, because a collection and a page may share a name and
 * are different things. A single page is one item whose identity is its name, so a relation to one
 * never holds an item id.
 */
export type RelationTarget =
  { kind: 'collection'; name: string } | { kind: 'single_page'; name: string };

/**
 * What a relation field is told, matching the Rust `RelationOptions`.
 *
 * A reference is a *set*: the order an editor sent does not survive, and the same reference twice
 * is one reference.
 */
export type RelationOptions = {
  target: RelationTarget;
  /** Whether several may be referenced at once. Always false for a single page. */
  has_many?: boolean;
  /** What the other side is called in screens and in `?populate=`. A label, not a second field. */
  inverse_name?: string;
};

export type RelationFieldSchema = {
  Relation: RelationOptions;
};

export type FieldTypeMap = {
  Text: TextFieldSchema;
  Slug: SlugFieldSchema;
  Markdown: MarkdownFieldSchema;
  Number: 'Number';
  Boolean: 'Boolean';
  Date: 'Date';
  DateTime: 'DateTime';
  Image: 'Image';
  CompositeField: CompositeFieldType;
  Relation: RelationFieldSchema;
  Array: ArrayFieldSchema;
  TextEnum: EnumFieldSchema;
};

export const FieldDefaults: FieldTypeMap = {
  Text: { Text: {} },
  Slug: { Slug: {} },
  Markdown: { Markdown: {} },
  Number: 'Number',
  Boolean: 'Boolean',
  Date: 'Date',
  DateTime: 'DateTime',
  Image: 'Image',
  CompositeField: { CompositeField: { id: '' } },
  // A new relation starts as "points at a collection", with the name still to be chosen - the
  // server refuses a target that does not exist, so the choice has to be made before a save.
  Relation: { Relation: { target: { kind: 'collection', name: '' }, has_many: false } },
  Array: { Array: [] },
  TextEnum: { TextEnum: [] },
};
export type FieldType = (typeof FieldDefaults)[keyof typeof FieldDefaults];

/**
 * `width` and `height` are required by the backend (`FieldSchema` in
 * `backend/crates/core/src/models/schema.rs` declares them as non-optional `u32`), so a schema saved
 * without them is rejected. `width` is a colspan (1-12) and `height` a rowspan (1-).
 */
export type FieldSchema = {
  name: string;
  field_type: FieldType;
  required: boolean;
  /**
   * Whether the value has to be unique inside its collection.
   *
   * Omitted by the server when false, so treat a missing value as false. Only a text field may
   * set it: the check compares the stored value, and a slug's normalisation is a separate job.
   */
  unique?: boolean;
  /**
   * Whether the collection's list shows this field as a column.
   *
   * Omitted by the server when false, so treat a missing value as false. A schema that marks no
   * field shows them all, which is what the list did before this existed - see the list screen's
   * `listColumns`. Only a collection has a list, so a page's or a composite's schema never sets it.
   */
  show_in_list?: boolean;
  /**
   * Whether this field is what a **reference** to this item shows as its name.
   *
   * Omitted by the server when false, so treat a missing value as false. One field per schema may
   * set it, and only a field that reads as one line (the server refuses the rest): a reference is
   * stored as which item it points at, and an id says nothing to a reader.
   */
  is_title?: boolean;
  width: number;
  height: number;
};

/**
 * The layout a newly added field starts with: full width, single row.
 *
 * `show_in_list` is false, so a field the author has just added stays out of the collection's list
 * until they say otherwise - adding a field is not the same act as deciding what identifies an item.
 */
export const DefaultFieldLayout = {
  width: 12,
  height: 1,
  unique: false,
  show_in_list: false,
} as const;

/**
 * A fresh copy of a default field type.
 *
 * `FieldDefaults` holds one object per type, and a field editor mutates the type it is handed -
 * the text limits are typed straight into it, the enum options and array item types are written
 * back into it. Handing that shared object to a field would make every field of the type share
 * one set of options: setting a maximum length on one text field would set it on all of them, and
 * on every text field added afterwards. Each field gets its own copy.
 */
export function newFieldType(type: keyof FieldTypeMap): FieldType {
  // JSON rather than `structuredClone`: the default is plain JSON, and this works in the browser
  // and in the tests alike.
  return JSON.parse(JSON.stringify(FieldDefaults[type])) as FieldType;
}

/**
 * A length as it goes on the wire: a whole number, or `undefined` for "no limit".
 *
 * The schema editor's inputs hand back whatever the form had - a string while it is being typed,
 * `''` when it is cleared - and the server deserialises `Option<usize>` strictly, so a string
 * there is a 422 about the JSON body rather than anything a reader could act on. Text limits are
 * normalised to numbers before a schema is saved.
 */
export function numberOrUndefined(value: unknown): number | undefined {
  if (value === null || value === undefined || value === '') {
    return undefined;
  }
  const parsed = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return undefined;
  }
  return Math.trunc(parsed);
}

/**
 * The schema as it is saved: the text limits of every field as numbers.
 *
 * A field that keeps its limits as strings would be refused by the server with a message about
 * deserialisation, which is nothing an editor can act on; normalising here means the screens
 * cannot save a schema the server will not read.
 */
export function schemaForSaving(fields: FieldSchema[]): FieldSchema[] {
  return fields.map((field) => {
    const type = field.field_type;
    if (isTextFieldSchema(type)) {
      return {
        ...field,
        field_type: {
          Text: {
            max_length: numberOrUndefined(type.Text.max_length),
            min_length: numberOrUndefined(type.Text.min_length),
          },
        },
      };
    }
    if (isMarkdownFieldSchema(type)) {
      return {
        ...field,
        field_type: {
          Markdown: {
            max_length: numberOrUndefined(type.Markdown.max_length),
            min_length: numberOrUndefined(type.Markdown.min_length),
          },
        },
      };
    }
    if (isRelationFieldSchema(type)) {
      const { target, has_many, inverse_name } = type.Relation;
      const inverse = inverse_name?.trim();
      return {
        ...field,
        field_type: {
          Relation: {
            target,
            // A single page is one item, so it can only ever be a single reference; the server
            // refuses anything else, and the editor's checkbox is disabled there.
            has_many: target.kind === 'single_page' ? false : Boolean(has_many),
            // A blank name is not a label: the server refuses it, so it is left off the wire.
            ...(inverse ? { inverse_name: inverse } : {}),
          },
        },
      };
    }
    return field;
  });
}

export function isTextFieldSchema(field: FieldType): field is TextFieldSchema {
  return typeof field === 'object' && field !== null && 'Text' in field;
}

export function isSlugFieldSchema(field: FieldType): field is SlugFieldSchema {
  return typeof field === 'object' && field !== null && 'Slug' in field;
}

export function isMarkdownFieldSchema(field: FieldType): field is MarkdownFieldSchema {
  return typeof field === 'object' && field !== null && 'Markdown' in field;
}

export function isCompositeFieldSchema(field: FieldType): field is CompositeFieldType {
  return typeof field === 'object' && field !== null && 'CompositeField' in field;
}
export function isArrayFieldSchema(field: FieldType): field is ArrayFieldSchema {
  return typeof field === 'object' && field !== null && 'Array' in field;
}
export function isRelationFieldSchema(field: FieldType): field is RelationFieldSchema {
  return typeof field === 'object' && field !== null && 'Relation' in field;
}
export function isEnumFieldSchema(field: FieldType): field is EnumFieldSchema {
  return typeof field === 'object' && field !== null && 'TextEnum' in field;
}

/**
 * Field types that can appear as array items.
 *
 * Text/Markdown/Array/TextEnum/CompositeField are omitted because they carry
 * configuration of their own that this editor does not collect yet.
 *
 * `Image` is allowed, but not together with `Number`: array items carry no type tag and
 * an image id is a JSON number, so a bare number would be ambiguous. Either on its own
 * is unambiguous. [`reconcileArrayItemTypes`] enforces that in the UI, and the server
 * rejects the combination too.
 */
export const ArrayItemTypeOptions: { label: string; value: FieldType }[] = [
  { label: 'Number', value: 'Number' },
  { label: 'Boolean', value: 'Boolean' },
  { label: 'Date', value: 'Date' },
  { label: 'DateTime', value: 'DateTime' },
  { label: 'Image', value: 'Image' },
];

/**
 * Apply the Number/Image exclusivity rule when the user changes an array's item types.
 *
 * Returns `selected` unchanged unless it contains both, in which case the type the user
 * just added wins and the other is dropped (adding `Image` to a `Number` array replaces
 * `Number`, and vice versa).
 */
export function reconcileArrayItemTypes(previous: FieldType[], selected: FieldType[]): FieldType[] {
  if (!(selected.includes('Number') && selected.includes('Image'))) {
    return selected;
  }
  // Whichever of the two the user just added wins; the other is dropped.
  const imageJustAdded = !previous.includes('Image') && selected.includes('Image');
  const dropped: FieldType = imageJustAdded ? 'Number' : 'Image';
  return selected.filter((type) => type !== dropped);
}

@Pipe({
  name: 'isTextFieldSchema',
})
export class IsTextFieldSchema implements PipeTransform {
  transform(field: FieldType): field is TextFieldSchema {
    return isTextFieldSchema(field);
  }
}
@Pipe({
  name: 'isMarkdownFieldSchema',
})
export class IsMarkdownFieldSchema implements PipeTransform {
  transform(field: FieldType): field is MarkdownFieldSchema {
    return isMarkdownFieldSchema(field);
  }
}
@Pipe({
  name: 'isCompositeFieldSchema',
})
export class IsCompositeFieldSchemaPipe implements PipeTransform {
  transform(field: FieldType): field is CompositeFieldType {
    return isCompositeFieldSchema(field);
  }
}
@Pipe({
  name: 'isArrayFieldSchema',
})
export class IsArrayFieldSchemaPipe implements PipeTransform {
  transform(field: FieldType): field is ArrayFieldSchema {
    return isArrayFieldSchema(field);
  }
}
@Pipe({
  name: 'isEnumFieldSchema',
})
export class IsEnumFieldSchemaPipe implements PipeTransform {
  transform(field: FieldType): field is EnumFieldSchema {
    return isEnumFieldSchema(field);
  }
}
@Pipe({
  name: 'isRelationFieldSchema',
})
export class IsRelationFieldSchemaPipe implements PipeTransform {
  transform(field: FieldType): field is RelationFieldSchema {
    return isRelationFieldSchema(field);
  }
}
@Pipe({
  name: 'FieldTypeString',
})
export class FieldTypeStringPipe implements PipeTransform {
  transform(field: FieldType): string {
    if (typeof field === 'string') {
      return field;
    } else if (isTextFieldSchema(field)) {
      return 'Text';
    } else if (isMarkdownFieldSchema(field)) {
      return 'Markdown';
    } else if (isCompositeFieldSchema(field)) {
      return 'CompositeField';
    } else if (isRelationFieldSchema(field)) {
      return 'Relation';
    } else if (isArrayFieldSchema(field)) {
      return 'Array';
    } else if (isEnumFieldSchema(field)) {
      return 'TextEnum';
    }
    return 'Unknown';
  }
}
