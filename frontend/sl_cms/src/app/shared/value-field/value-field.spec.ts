import { provideHttpClient } from '@angular/common/http';
import { By } from '@angular/platform-browser';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { of } from 'rxjs';

import { CompositeFieldDefinition } from 'app/models/schema/collection';
import { FieldSchema, FieldType } from 'app/models/schema/fields';
import { FieldValue } from 'app/models/values/fields';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { CompositeFieldsService } from 'app/services/schema/composite_fields.service';
import { Message, t } from 'app/core/i18n/message';
import { ImagesService } from 'app/services/media/images.service';
import { ValueField } from './value-field';

function field(
  name: string,
  field_type: FieldType,
  layout: Partial<FieldSchema> = {},
): FieldSchema {
  return { name, field_type, required: false, width: 12, height: 1, ...layout };
}

/** The composite definitions the component reads, keyed by id. */
const COMPOSITE_DEFINITIONS: { [id: string]: CompositeFieldDefinition } = {
  seo: [field('description', { Text: {} })],
  gallery: [field('images', { Array: ['Image'] })],
  // Parts with a layout of their own: the content editor has to lay them out the way the
  // schema editor drew them, or the two screens disagree about the same definition.
  layout: [
    field('headline', { Text: {} }, { width: 8, height: 2 }),
    field('aside', { Text: {} }, { width: 4 }),
  ],
  // A block that holds blocks: the definition reaches itself through an array.
  tree: [field('line', { Text: {} }), field('children', { Array: [{ CompositeField: { id: 'tree' } }] })],
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
    return of({ id: 100 + index, upload_url: `/images/${file.name}?key=k`, url: `/images/${file.name}` });
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

  it('lays a composite’s sub-fields out with the widths and heights of the definition', () => {
    create(field('page', { CompositeField: { id: 'layout' } }), {
      id: 'layout',
      values: { headline: 'Hello', aside: 'Sidebar' },
    });
    fixture.detectChanges();

    const cells = Array.from(
      fixture.nativeElement.querySelectorAll('fieldset.composite .field-cell'),
    ) as HTMLElement[];
    expect(cells.length).toBe(2);
    expect(cells[0].style.gridColumn).toBe('span 8');
    expect(cells[0].style.minHeight).toBe('calc(var(--field-row-unit, 72px) * 2)');
    expect(cells[1].style.gridColumn).toBe('span 4');
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

  // The schema's lengths used to be the schema editor's business only: the content editor let a
  // value past them and the server refused the save after the fact.
  it('states the lengths the schema set, and counts what has been typed', async () => {
    create(field('title', { Text: { max_length: 5, min_length: 2 } }), 'abc');
    await fixture.whenStable();
    fixture.detectChanges();

    const input = query('input[matinput]') as HTMLInputElement;
    expect(input.getAttribute('maxlength')).toBe('5');
    expect(input.getAttribute('minlength')).toBe('2');
    expect(fixture.nativeElement.textContent).toContain('2–5 characters');
    expect(fixture.nativeElement.textContent).toContain('3 / 5');
  });

  it('reports a value outside the lengths before the save, and clears it again', () => {
    const component = create(field('title', { Text: { max_length: 5, min_length: 2 } }), 'abc');
    const problems: (Message | null)[] = [];
    component.errorChange.subscribe((problem) => problems.push(problem));

    component.update('abcdef');
    expect(problems[0]).toEqual(t('content.valueTooLong', { field: 'title', max: 5 }));

    component.update('a');
    expect(problems[1]).toEqual(t('content.valueTooShort', { field: 'title', min: 2 }));

    component.update('ab');
    expect(problems[2]).toBeNull();
  });

  // Characters, not bytes: five Japanese characters are five, which is what the server counts
  // too, and what a schema that says 5 has to mean.
  it('counts characters rather than bytes', () => {
    const component = create(field('title', { Text: { max_length: 5 } }), '');
    const problems: (Message | null)[] = [];
    component.errorChange.subscribe((problem) => problems.push(problem));

    component.update('あいうえお');
    expect(problems).toEqual([null]);

    component.update('あいうえおか');
    expect(problems[1]).toEqual(t('content.valueTooLong', { field: 'title', max: 5 }));
  });

  it('names the item types a scalar array accepts', async () => {
    // The JSON box is the whole editor for these, so what it accepts has to be on the screen.
    create(field('tags', { Array: [{ Text: {} }, 'Number'] }));
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.nativeElement.textContent).toContain('Item types: Text, Number');
  });

  // An array of composites is edited element by element, so the element's own fields say what it
  // holds; the JSON view is the one that has to name the definition.
  it('names the composite definition an array of composites holds, in the JSON view', async () => {
    const component = create(field('blocks', { Array: [{ CompositeField: { id: 'seo' } }] }));
    component.jsonMode.set(true);
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.nativeElement.textContent).toContain('Item types: seo');
  });

  // The server tries each declared type in turn; this only rules out what none of them could
  // read, and it says so while the reader is looking at the box rather than after a save.
  it('refuses an item no declared item type could read', () => {
    const component = create(field('scores', { Array: ['Number'] }), []);
    const problems: (Message | null)[] = [];
    component.errorChange.subscribe((problem) => problems.push(problem));

    component.onArrayTextChange('[1, "two", 3]');

    expect(problems[0]).toEqual(
      t('content.arrayItemType', { field: 'scores[1]', types: 'Number' }),
    );
    // The last good value is kept, as it is for JSON that does not parse.
    expect(component.value).toEqual([]);
  });

  it('accepts what a declared item type could read, without second-guessing the server', () => {
    const tags = create(field('tags', { Array: [{ Text: {} }] }), []);
    const textProblems: (Message | null)[] = [];
    tags.errorChange.subscribe((problem) => textProblems.push(problem));
    tags.onArrayTextChange('["one", 2]');
    expect(textProblems[0]).not.toBeNull();

    // A date-shaped string is a string: an array of texts takes it.
    const dates = create(field('notes', { Array: [{ Text: {} }] }), []);
    const dateProblems: (Message | null)[] = [];
    dates.errorChange.subscribe((problem) => dateProblems.push(problem));
    dates.onArrayTextChange('["2026-01-01"]');
    expect(dateProblems).toEqual([null]);
    expect(dates.value).toEqual(['2026-01-01']);
  });

  it('says a field has to be unique, which only the server can check', () => {
    create(field('slug', { Text: {} }, { unique: true }));
    expect(query('.unique-mark')?.textContent?.trim()).toBe('unique');

    create(field('title', { Text: {} }));
    expect(query('.unique-mark')).toBeFalsy();
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
    const errors: (Message | null)[] = [];
    component.valueChange.subscribe((value) => values.push(value));
    component.errorChange.subscribe((error) => errors.push(error));

    component.onArrayTextChange('[1, 2');
    expect(errors.at(-1)).toEqual(t('content.invalidJson', { field: 'numbers' }));
    // Nothing is emitted, so the previously valid value is what the parent still holds.
    expect(values).toHaveLength(0);

    component.onArrayTextChange('{"not":"an array"}');
    expect(errors.at(-1)).toEqual(t('content.expectedJsonArray', { field: 'numbers' }));

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

  it('stores the URL the server reports, not one derived from the upload URL', () => {
    // On AWS the upload URL is a presigned request to S3: it names a signature and an
    // expiry, and trimming its query string yields nothing that can be read later. The
    // server answers with the stable address as well, and that is what content keeps.
    const presigned =
      'https://cms-images.s3.eu-west-1.amazonaws.com/9f3c.png?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Signature=deadbeef';
    const stable = 'https://images.example.com/9f3c.png';
    images.uploadImage = () => of({ id: 7, upload_url: presigned, url: stable });

    const component = create(field('cover', { Array: ['Image'] }), []);
    const changes: FieldValue[] = [];
    component.valueChange.subscribe((value) => changes.push(value));

    component.onFilesSelected(filesChosen(['cover.png']));
    fixture.detectChanges();

    expect(changes).toEqual([[{ id: 7, url: stable }]]);
    expect(JSON.stringify(changes)).not.toContain('X-Amz-Signature');
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

  it('edits a composite array element by element', () => {
    const component = create(field('blocks', { Array: [{ CompositeField: { id: 'seo' } }] }), [
      { id: 'seo', values: { description: 'first' } },
    ] as unknown as FieldValue);
    // A second pass, as the other composite tests do: the sub-editor resolves its definition
    // while the first one runs.
    fixture.detectChanges();

    expect(component.isCompositeArray()).toBe(true);
    // The element gets the composite's own editor with the stored value, not a JSON box.
    const element = fixture.nativeElement.querySelector('.composite-element') as HTMLElement;
    expect(element).toBeTruthy();
    expect(element.textContent).toContain('Element 1');

    // The element is edited by the definition's own editor, and it was handed the stored
    // sub-values rather than the read wrapper, which the storage layer unwraps.
    // The first descendant editor is the element's; `queryAll` does not include the root, so
    // the root's own editor is not in this list.
    const editor = fixture.debugElement.queryAll(By.directive(ValueField))[0]
      .componentInstance as ValueField;
    expect(editor.field?.field_type).toEqual({ CompositeField: { id: 'seo' } });
    expect(editor.compositeValues).toEqual({ description: 'first' });
  });

  it('adds an element with its definition defaults, and removes it again', () => {
    const component = create(field('blocks', { Array: [{ CompositeField: { id: 'seo' } }] }), []);
    const changes: FieldValue[] = [];
    component.valueChange.subscribe((value) => changes.push(value));

    component.addElement();
    expect(changes.at(-1)).toEqual([{ id: 'seo', values: { description: '' } }]);

    /** What a parent does with an emission: hand it straight back as the input. */
    const accept = () => {
      fixture.componentRef.setInput('value', changes.at(-1));
      fixture.detectChanges();
    };

    accept();
    expect(fixture.nativeElement.querySelectorAll('.composite-element').length).toBe(1);

    component.addElement();
    accept();
    expect(fixture.nativeElement.querySelectorAll('.composite-element').length).toBe(2);

    component.removeElementAt(0);
    accept();
    expect(fixture.nativeElement.querySelectorAll('.composite-element').length).toBe(1);
    expect((changes.at(-1) as unknown[]).length).toBe(1);
  });

  it('keeps the definition each element names, and their order', () => {
    const component = create(
      field('blocks', {
        Array: [{ CompositeField: { id: 'seo' } }, { CompositeField: { id: 'gallery' } }],
      }),
      [
        { id: 'gallery', values: { images: [] } },
        { id: 'seo', values: { description: 'second' } },
      ] as unknown as FieldValue,
    );
    fixture.detectChanges();

    // One element per definition, each edited by that definition's schema.
    expect(component.elementItems.map((element) => element.id)).toEqual(['gallery', 'seo']);
    expect(component.elementSchemas[0].field_type).toEqual({ CompositeField: { id: 'gallery' } });

    component.moveElement(0, 1);

    expect(component.value).toEqual([
      { id: 'seo', values: { description: 'second' } },
      { id: 'gallery', values: { images: [] } },
    ]);
  });

  it('stores what an element editor emits under the definition the element names', () => {
    const component = create(field('blocks', { Array: [{ CompositeField: { id: 'seo' } }] }), []);
    const changes: FieldValue[] = [];
    component.valueChange.subscribe((value) => changes.push(value));

    component.addElement();
    component.setElementValues(0, { description: 'typed' } as unknown as FieldValue);

    expect(changes.at(-1)).toEqual([{ id: 'seo', values: { description: 'typed' } }]);
  });

  it('stops where the value stops when a definition holds an array of itself', () => {
    // The editor draws array elements from the value and composite sub-fields from the schema, so
    // this is where a self-referencing definition ends: one level per stored element, and an
    // element whose own array is empty draws nothing. (That is also what makes the schema legal.)
    create(field('blocks', { Array: [{ CompositeField: { id: 'tree' } }] }), [
      { id: 'tree', values: { line: 'root', children: [] } },
    ] as unknown as FieldValue);
    fixture.detectChanges();

    expect(fixture.nativeElement.querySelectorAll('.composite-element').length).toBe(1);
    expect(fixture.nativeElement.querySelectorAll('.composite-element .composite-element').length).toBe(0);
    expect(
      (fixture.nativeElement.querySelector('.composite-element .note') as HTMLElement)?.textContent,
    ).toContain('No elements yet');

    // The same field with one element stored at the second level draws exactly that much.
    create(field('blocks', { Array: [{ CompositeField: { id: 'tree' } }] }), [
      {
        id: 'tree',
        values: {
          line: 'root',
          children: [{ id: 'tree', values: { line: 'leaf', children: [] } }],
        },
      },
    ] as unknown as FieldValue);
    fixture.detectChanges();

    expect(fixture.nativeElement.querySelectorAll('.composite-element').length).toBe(2);
    expect(fixture.nativeElement.querySelectorAll('.composite-element .composite-element').length).toBe(1);
  });

  it('leaves a mixed array to the JSON view, which accepts composite elements', () => {
    // Nothing in a bare element says which of the two editors it needs, so guessing is not an
    // option; the JSON view is unambiguous and the server reads composites there.
    const component = create(
      field('mixed', { Array: ['Number', { CompositeField: { id: 'seo' } }] }),
      [],
    );

    expect(component.isCompositeArray()).toBe(false);
    expect(query('textarea')).toBeTruthy();
    expect(query('.composite-element')).toBeNull();
  });
});
