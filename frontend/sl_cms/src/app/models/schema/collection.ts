import { FieldSchema } from './fields';

export type CollectionSchema = Array<FieldSchema>;

/**
 * A composite field's definition: the list of field schemas its value is made of. The
 * same shape as a collection schema.
 *
 * Distinct from `CompositeFieldType`, which is the *field type* that points at one of
 * these by id.
 */
export type CompositeFieldDefinition = CollectionSchema;
