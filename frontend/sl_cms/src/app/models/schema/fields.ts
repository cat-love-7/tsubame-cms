import { Pipe, PipeTransform } from "@angular/core";

export type TextFieldOptions = {
  max_length?: number;
  min_length?: number;
}

export type TextFieldSchema = {
  Text: TextFieldOptions;
}
export type MarkdownFieldSchema = {
  Markdown: TextFieldOptions;
}

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
}

/**
 * An array element is a bare field *type*, matching the Rust
 * `FieldType::Array(Vec<FieldType>)` (it is not a full `FieldSchema`, which would also
 * carry a name and a required flag).
 */
export type ArrayFieldSchema = {
  Array: FieldType[];
}

/**
 * Enum options travel as a JSON array on the wire (`{"TextEnum":["a","b"]}`). A `Set`
 * would serialise to `{}`, so this must stay an array.
 */
export type EnumFieldSchema = {
  TextEnum: string[];
}

export type FieldTypeMap = {
  Text: TextFieldSchema,
  Markdown: MarkdownFieldSchema,
  Number: 'Number',
  Boolean: 'Boolean',
  Date: 'Date',
  DateTime: 'DateTime',
  Image: 'Image',
  CompositeField: CompositeFieldType,
  Array: ArrayFieldSchema,
  TextEnum: EnumFieldSchema,
};

export const FieldDefaults: FieldTypeMap = {
  Text: { Text: {} },
  Markdown: { Markdown: {} },
  Number: 'Number',
  Boolean: 'Boolean',
  Date: 'Date',
  DateTime: 'DateTime',
  Image: 'Image',
  CompositeField: { CompositeField: { id: '' } },
  Array: { Array: [] },
  TextEnum: { TextEnum: [] },
};
export type FieldType = typeof FieldDefaults[keyof typeof FieldDefaults];

/**
 * `width` and `height` are required by the backend (`FieldSchema` in
 * `sl_cms/src/models/schema.rs` declares them as non-optional `u32`), so a schema saved
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
  width: number;
  height: number;
}

/** The layout a newly added field starts with: full width, single row. */
export const DefaultFieldLayout = { width: 12, height: 1, unique: false } as const;

export function isTextFieldSchema(field: FieldType): field is TextFieldSchema {
  return typeof field === 'object' && field !== null && 'Text' in field;
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
export function reconcileArrayItemTypes(
  previous: FieldType[],
  selected: FieldType[],
): FieldType[] {
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
    } else if (isArrayFieldSchema(field)) {
      return 'Array';
    } else if (isEnumFieldSchema(field)) {
      return 'TextEnum';
    }
    return 'Unknown';
  }
}
