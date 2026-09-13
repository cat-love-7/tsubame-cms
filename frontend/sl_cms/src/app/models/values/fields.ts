
export type TextField = {
  Text: string;
}
export type MarkdownField = {
  Markdown: string;
}
export type NumberField = {
  Number: number;
}
export type BooleanField = {
  Boolean: boolean;
}
export type DateField = {
  Date: string;
}
export type DateTimeField = {
  DateTime: string;
}
export type ImageField = {
  Image: number; // Image ID
}
export type CompositeField = {
  CompositeField: { [key: string]: FieldValue };
}

export type ArrayColumnType = Exclude<FieldValue, ArrayField>;
export type ArrayField = {
  Array: ArrayColumnType[];
}
export type EnumField = {
  TextEnum: string[];
}

export type FieldValue = TextField | MarkdownField | NumberField | BooleanField | DateField | DateTimeField | ImageField | CompositeField | ArrayField | EnumField | null;

