import { of } from 'rxjs';

import { CompositeFieldDefinition } from 'app/models/schema/collection';
import { FieldSchema, FieldType } from 'app/models/schema/fields';
import { ImageEntry } from 'app/repositories/media/images.repository';

/**
 * The values and doubles the field widgets' specs share.
 *
 * A field value is edited by one widget and laid out by the component above it, and both have
 * specs: the widget's own, and the render of it through the component above. They describe the
 * same schema and the same library, so those live here rather than in one of the two.
 */
export function field(
  name: string,
  field_type: FieldType,
  layout: Partial<FieldSchema> = {},
): FieldSchema {
  return { name, field_type, required: false, width: 12, height: 1, ...layout };
}

/** The composite definitions the components read, keyed by id. */
export const COMPOSITE_DEFINITIONS: { [id: string]: CompositeFieldDefinition } = {
  seo: [field('description', { Text: {} })],
  gallery: [field('images', { Array: ['Image'] })],
  // Parts with a layout of their own: the content editor has to lay them out the way the
  // schema editor drew them, or the two screens disagree about the same definition.
  layout: [
    field('headline', { Text: {} }, { width: 8, height: 2 }),
    field('aside', { Text: {} }, { width: 4 }),
  ],
  // A definition that holds a relation: the target is a collection of the site, and the field that
  // embeds the definition is what declares it.
  cta: [
    field('author', {
      Relation: { target: { kind: 'collection', name: 'authors' }, has_many: true },
    }),
  ],
  // A block that holds blocks: the definition reaches itself through an array.
  tree: [
    field('line', { Text: {} }),
    field('children', { Array: [{ CompositeField: { id: 'tree' } }] }),
  ],
};

export const LIBRARY: ImageEntry[] = [
  {
    id: 3,
    url: '/images/logo.png',
    original_filename: 'logo.png',
    uploaded_at: '2024-01-01T00:00:00Z',
  },
  {
    id: 4,
    url: '/images/photo.png',
    original_filename: 'photo.png',
    uploaded_at: '2024-01-02T00:00:00Z',
  },
];

/** Counts library reads, so the picker can be checked for loading lazily. */
export class StubImagesService {
  public listCalls = 0;
  /** Names of the files handed to `uploadImage`, in order. */
  public uploaded: string[] = [];
  listImages = () => {
    this.listCalls += 1;
    // The page shape the server answers with: the images, and how many there are altogether.
    return of({ images: LIBRARY, total: LIBRARY.length });
  };
  uploadImage = (file: File) => {
    const index = this.uploaded.push(file.name);
    return of({
      id: 100 + index,
      upload_url: `/images/${file.name}?key=k`,
      url: `/images/${file.name}`,
    });
  };
  deleteImage = () => of(void 0);
  /** The durable link to an image, which is what the Markdown toolbar inserts. */
  imageLink = (id: number) => `/images/by-id/${id}`;
}

/** A change event for a file input that was given several files. */
export function filesChosen(names: string[]): Event {
  const input = {
    files: names.map((name) => new File(['x'], name, { type: 'image/png' })),
    value: '',
  };
  return { target: input } as unknown as Event;
}
