import { Pipe, PipeTransform } from "@angular/core";
import { FieldValue } from "../values/fields";

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

export type CompositeFieldSchema = {
  CompositeField: {
    id: string;
  };
}

export type ArrayColumnType = Exclude<FieldSchema, ArrayFieldSchema>;

export type ArrayFieldSchema = {
  Array: ArrayColumnType[];
}

export type EnumFieldSchema = {
  TextEnum: Set<string>;
}

export type FieldTypeMap = {
  Text: TextFieldSchema,
  Markdown: MarkdownFieldSchema,
  Number: 'Number',
  Boolean: 'Boolean',
  Date: 'Date',
  DateTime: 'DateTime',
  Image: 'Image',
  CompositeField: CompositeFieldSchema,
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
  TextEnum: { TextEnum: new Set<string>() },
};
export type FieldType = typeof FieldDefaults[keyof typeof FieldDefaults];

export type FieldSchema = {
  name: string;
  field_type: FieldType;
  required: boolean;
}

export function isTextFieldSchema(field: FieldType): field is TextFieldSchema {
  return typeof field === 'object' && field !== null && 'Text' in field;
}

export function isMarkdownFieldSchema(field: FieldType): field is MarkdownFieldSchema {
  return typeof field === 'object' && field !== null && 'Markdown' in field;
}

export function isCompositeFieldSchema(field: FieldType): field is CompositeFieldSchema {
  return typeof field === 'object' && field !== null && 'CompositeField' in field;
}
export function isArrayFieldSchema(field: FieldType): field is ArrayFieldSchema {
  return typeof field === 'object' && field !== null && 'Array' in field;
}
export function isEnumFieldSchema(field: FieldType): field is EnumFieldSchema {
  return typeof field === 'object' && field !== null && 'TextEnum' in field;
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
  transform(field: FieldType): field is CompositeFieldSchema {
    return isCompositeFieldSchema(field);
  }
}
@Pipe({
  name: 'isArrayFieldSchema',
})
export class IsArrayFieldSchemaSchemaPipe implements PipeTransform {
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

export function getValueTypeFromFieldSchema(fields: Array<FieldSchema>): { [key: string]: FieldValue} {
  let result: { [key: string]: FieldValue } = {};
  for (const field of fields) {
    {
      switch (field.field_type) {
        case 'Number':
          result[field.name] = { Number: 0 };
          break;
        case 'Boolean':
          result[field.name] = { Boolean: false };
          break;
        case 'Date':
          result[field.name] = { Date: '' };
          break;
        case 'DateTime':
          result[field.name] = { DateTime: '' };
          break;
        case 'Image':
          result[field.name] = { Image: 0 };
          break;
        default:
          if (isTextFieldSchema(field.field_type)) {
            result[field.name] = { Text: '' };
          } else if (isMarkdownFieldSchema(field.field_type)) {
            result[field.name] = { Markdown: '' };
          } else if (isCompositeFieldSchema(field.field_type)) {
            result[field.name] = { CompositeField: getValueTypeFromFieldSchema([]) };
          } else if (isArrayFieldSchema(field.field_type)) {
            result[field.name] = { Array: [] };
          } else if (isEnumFieldSchema(field.field_type)) {
            result[field.name] = { TextEnum: [] };
          }
      }
    }
  }
  return result;
}