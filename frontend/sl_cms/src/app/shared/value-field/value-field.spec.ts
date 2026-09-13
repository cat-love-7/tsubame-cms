import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { of } from 'rxjs';

import { CompositeFieldDefinition } from 'app/models/schema/collection';
import { FieldSchema, FieldType } from 'app/models/schema/fields';
import { FieldValue } from 'app/models/values/fields';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { CompositeFieldsService } from 'app/services/schema/composite_fields.service';
import { ImagesService } from 'app/services/media/images.service';
import { ValueField } from './value-field';

function field(name: string, field_type: FieldType): FieldSchema {
  return { name, field_type, required: false, width: 12, height: 1 };
}

/** The composite definitions the component reads, keyed by id. */
const COMPOSITE_DEFINITIONS: { [id: string]: CompositeFieldDefinition } = {
  seo: [field('description', { Text: {} })],
  gallery: [field('images', { Array: ['Image'] })],
};

const LIBRARY: ImageEntry[] = [
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
class StubImagesService {
  public listCalls = 0;
  /** Names of the files handed to `uploadImage`, in order. */
  public uploaded: string[] = [];
  listImages = () => {
    this.listCalls += 1;
    return of(LIBRARY);
  };
  uploadImage = (file: File) => {
    const index = this.uploaded.push(file.name);
    return of({ id: 100 + index, upload_url: `/images/${file.name}?key=k` });
  };
  deleteImage = () => of(void 0);
}

/** A change event for a file input that was given several files. */
function filesChosen(names: string[]): Event {
  const input = {
    files: names.map((name) => new File(['x'], name, { type: 'image/png' })),
    value: '',
  };
  return { target: input } as unknown as Event;
}

describe('ValueField', () => {
  let fixture: ComponentFixture<ValueField>;
  let images: StubImagesService;

  beforeEach(async () => {
    images = new StubImagesService();
    await TestBed.configureTestingModule({
      imports: [ValueField],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        // A stub rather than the HTTP-backed service, so the composite definitions are
        // available synchronously.
        {
          provide: CompositeFieldsService,
          useValue: { getAllCompositeFields: () => of(COMPOSITE_DEFINITIONS) },
        },
        { provide: ImagesService, useValue: images },
      ],
    }).compileComponents();
  });

  /** Uses `setInput` so the inputs are in place before the first change detection. */
  function create(fieldSchema: FieldSchema, value: FieldValue = null): ValueField {
    fixture = TestBed.createComponent(ValueField);
    fixture.componentRef.setInput('field', fieldSchema);
    fixture.componentRef.setInput('value', value);
    fixture.detectChanges();
    return fixture.componentInstance;
  }

  function query(selector: string): Element | null {
    return fixture.nativeElement.querySelector(selector);
  }

  it('renders the control the field type calls for', () => {
    create(field('text', { Text: {} }));
    expect(query('input[matinput]')).toBeTruthy();

    create(field('body', { Markdown: {} }));
    expect(query('textarea')).toBeTruthy();

    create(field('count', 'Number'));
    expect(query('input[type="number"]')).toBeTruthy();

    create(field('live', 'Boolean'));
    expect(query('mat-checkbox')).toBeTruthy();

    create(field('published', 'Date'));
    expect(query('input[type="date"]')).toBeTruthy();

    create(field('at', 'DateTime'));
    expect(query('input[type="datetime-local"]')).toBeTruthy();

    create(field('tags', { TextEnum: ['a', 'b'] }));
    expect(query('mat-select')).toBeTruthy();

    create(field('numbers', { Array: ['Number'] }));
    expect(query('textarea')).toBeTruthy();
    expect(query('mat-select')).toBeFalsy();
  });

  it('explains that an undefined composite field cannot be edited rather than dropping it', () => {
    create(field('missing', { CompositeField: { id: 'nope' } }));
    expect(fixture.nativeElement.textContent).toContain('is not defined');
  });

  it('renders a composite field as its sub-fields, nested through this same component', () => {
    create(field('seo', { CompositeField: { id: 'seo' } }));

    // The sub-field is rendered by another ValueField instance: the recursion works.
    expect(fixture.nativeElement.querySelector('app-value-field')).toBeTruthy();
    expect(fixture.nativeElement.textContent).toContain('description');
  });

  it('emits the bare object for a composite value, unwrapping the read wrapper', () => {
    // Reads wrap a composite as `{id, values}`; the wrapper must not leak into sub-fields
    // or back out onto the wire.
    const component = create(
      field('seo', { CompositeField: { id: 'seo' } }),
      { id: 'seo', values: { description: 'meta' } },
    );
    const emitted: FieldValue[] = [];
    component.valueChange.subscribe((value) => emitted.push(value));

    component.setCompositeValue(field('description', { Text: {} }), 'changed');

    expect(emitted).toEqual([{ description: 'changed' }]);
  });

  it('emits a new value when the input changes', () => {
    const component = create(field('text', { Text: {} }), 'old');
    const emitted: FieldValue[] = [];
    component.valueChange.subscribe((value) => emitted.push(value));

    component.update('new');

    expect(emitted).toEqual(['new']);
    expect(component.value).toBe('new');
  });

  it('seeds the JSON buffer from the incoming array', () => {
    const component = create(field('numbers', { Array: ['Number'] }), [1, 2]);
    expect(component.arrayText).toBe('[1,2]');
  });

  // The parent hands the emitted value straight back, and re-seeding from it would fight
  // whatever the user is typing.
  it('does not reset the buffer when its own value comes back', () => {
    const component = create(field('numbers', { Array: ['Number'] }), []);

    component.onArrayTextChange('[1, 2]');
    expect(component.arrayText).toBe('[1, 2]');

    fixture.componentRef.setInput('value', component.value);
    fixture.detectChanges();

    expect(component.arrayText).toBe('[1, 2]');
  });

  it('reports invalid array JSON without emitting a value', () => {
    const component = create(field('numbers', { Array: ['Number'] }), []);
    const values: FieldValue[] = [];
    const errors: (string | null)[] = [];
    component.valueChange.subscribe((value) => values.push(value));
    component.errorChange.subscribe((error) => errors.push(error));

    component.onArrayTextChange('[1, 2');
    expect(errors.at(-1)).toContain('invalid JSON');
    // Nothing is emitted, so the previously valid value is what the parent still holds.
    expect(values).toHaveLength(0);

    component.onArrayTextChange('{"not":"an array"}');
    expect(errors.at(-1)).toContain('expected a JSON array');

    component.onArrayTextChange('[1, 2]');
    expect(errors.at(-1)).toBeNull();
    expect(values).toEqual([[1, 2]]);

    component.onArrayTextChange('');
    expect(values.at(-1)).toEqual([]);
  });

  it('converts date-times between local input and RFC 3339', () => {
    const component = create(field('at', 'DateTime'), '2024-03-01T10:00:00.000Z');

    // The exact local rendering depends on the test machine's timezone, so assert the
    // round trip rather than a literal string.
    const local = component.toDateTimeLocal();
    expect(local).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/);

    component.setDateTime('2024-03-01T10:00');
    expect(component.value).toBe(new Date('2024-03-01T10:00').toISOString());

    component.setDateTime('');
    expect(component.value).toBeNull();
  });

  it('loads the image library when the picker is opened, and only then', () => {
    const component = create(field('photo', 'Image'));
    expect(images.listCalls).toBe(0);

    component.openLibrary(false);
    expect(images.listCalls).toBe(1);

    // Closing and reopening reuses what was already fetched.
    component.closePicker();
    component.openLibrary(false);
    expect(images.listCalls).toBe(1);
  });

  it('picks an already uploaded image instead of uploading a new one', () => {
    const component = create(field('photo', 'Image'));
    const changes: FieldValue[] = [];
    component.valueChange.subscribe((value) => changes.push(value));

    component.openLibrary(false);
    fixture.detectChanges();

    const thumbs = fixture.nativeElement.querySelectorAll('.thumb') as NodeListOf<HTMLButtonElement>;
    expect(thumbs.length).toBe(2);

    thumbs[0].click();
    fixture.detectChanges();

    // The value keeps the shape an upload produces, so the server cannot tell them apart.
    expect(changes).toEqual([{ id: 3, url: '/images/logo.png' }]);
    expect(component.value).toEqual({ id: 3, url: '/images/logo.png' });
    expect(component.pickerOpen()).toBe(false);
  });

  it('shows an image array as thumbnails, not as JSON', () => {
    const component = create(field('covers', { Array: ['Image'] }), [
      { id: 3, url: '/images/logo.png' },
    ]);

    expect(component.isImageArray()).toBe(true);
    const items = fixture.nativeElement.querySelectorAll('.array-item');
    expect(items.length).toBe(1);
    expect(items[0].querySelector('img')?.getAttribute('src')).toBe('/api/images/logo.png');
    // The JSON editor is behind the toggle, not in the way.
    expect(query('textarea')).toBeFalsy();
  });

  it('keeps the JSON editor for arrays that are not images', () => {
    const component = create(field('numbers', { Array: ['Number'] }), [1, 2]);

    expect(component.isImageArray()).toBe(false);
    expect(query('textarea')).toBeTruthy();
    expect(fixture.nativeElement.querySelectorAll('.array-item').length).toBe(0);
  });

  it('adds several images from the library at once', () => {
    const component = create(field('covers', { Array: ['Image'] }), []);
    const changes: FieldValue[] = [];
    component.valueChange.subscribe((value) => changes.push(value));

    (query('button.array-add') as HTMLButtonElement).click();
    fixture.detectChanges();

    // Multi mode: clicking ticks rather than choosing, until the add button is pressed.
    const thumbs = fixture.nativeElement.querySelectorAll('.thumb') as NodeListOf<HTMLButtonElement>;
    thumbs[0].click();
    thumbs[1].click();
    fixture.detectChanges();

    expect(component.selected()).toEqual([3, 4]);
    expect(changes).toEqual([]);

    (query('button.array-add-selected') as HTMLButtonElement).click();
    fixture.detectChanges();

    expect(changes).toEqual([
      [
        { id: 3, url: '/images/logo.png' },
        { id: 4, url: '/images/photo.png' },
      ],
    ]);
    expect(fixture.nativeElement.querySelectorAll('.array-item').length).toBe(2);
    expect(component.pickerOpen()).toBe(false);
  });

  it('appends to the images an array already holds', () => {
    const component = create(field('covers', { Array: ['Image'] }), [
      { id: 4, url: '/images/photo.png' },
    ]);

    (query('button.array-add') as HTMLButtonElement).click();
    fixture.detectChanges();
    (fixture.nativeElement.querySelectorAll('.thumb')[0] as HTMLButtonElement).click();
    fixture.detectChanges();
    (query('button.array-add-selected') as HTMLButtonElement).click();
    fixture.detectChanges();

    expect(component.value).toEqual([
      { id: 4, url: '/images/photo.png' },
      { id: 3, url: '/images/logo.png' },
    ]);
  });

  it('reorders and removes images in an array', () => {
    const component = create(field('covers', { Array: ['Image'] }), [
      { id: 3, url: '/images/logo.png' },
      { id: 4, url: '/images/photo.png' },
    ]);

    (query('button[aria-label="move image 1 earlier"]') as HTMLButtonElement).click();
    expect(component.value).toEqual([
      { id: 4, url: '/images/photo.png' },
      { id: 3, url: '/images/logo.png' },
    ]);

    fixture.detectChanges();
    (query('button[aria-label="remove image 0"]') as HTMLButtonElement).click();
    expect(component.value).toEqual([{ id: 3, url: '/images/logo.png' }]);
  });

  it('keeps the JSON view in step with the thumbnails', () => {
    const component = create(field('covers', { Array: ['Image'] }), []);

    (query('button.array-add') as HTMLButtonElement).click();
    fixture.detectChanges();
    (fixture.nativeElement.querySelectorAll('.thumb')[0] as HTMLButtonElement).click();
    fixture.detectChanges();
    (query('button.array-add-selected') as HTMLButtonElement).click();
    fixture.detectChanges();

    expect(component.arrayText).toBe('[{"id":3,"url":"/images/logo.png"}]');
  });

  it('uploads files straight into an image array, in the order they were chosen', () => {
    const component = create(field('covers', { Array: ['Image'] }), []);
    const changes: FieldValue[] = [];
    component.valueChange.subscribe((value) => changes.push(value));

    component.onFilesSelected(filesChosen(['first.png', 'second.png']));
    fixture.detectChanges();

    expect(images.uploaded).toEqual(['first.png', 'second.png']);
    expect(changes).toEqual([
      [
        { id: 101, url: '/images/first.png' },
        { id: 102, url: '/images/second.png' },
      ],
    ]);
    expect(fixture.nativeElement.querySelectorAll('.array-item').length).toBe(2);
    expect(component.uploading()).toBe(false);
  });

  it('edits an image array that lives inside a composite', () => {
    const component = create(
      field('block', { CompositeField: { id: 'gallery' } }),
      { images: [{ id: 3, url: '/images/logo.png' }] } as unknown as FieldValue,
    );
    fixture.detectChanges();

    // Sub-fields are edited by this same component, so the nested array gets the same UI.
    const nested = fixture.nativeElement.querySelectorAll('fieldset.composite .array-item');
    expect(nested.length).toBe(1);

    (query('fieldset.composite button.array-add') as HTMLButtonElement).click();
    fixture.detectChanges();
    (fixture.nativeElement.querySelectorAll('fieldset.composite .thumb')[1] as HTMLButtonElement).click();
    fixture.detectChanges();
    (query('fieldset.composite button.array-add-selected') as HTMLButtonElement).click();
    fixture.detectChanges();

    expect(component.value).toEqual({
      images: [
        { id: 3, url: '/images/logo.png' },
        { id: 4, url: '/images/photo.png' },
      ],
    });
  });
});
