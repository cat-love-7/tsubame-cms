import { Component, EventEmitter, ViewChild } from '@angular/core';
import { provideHttpClient } from '@angular/common/http';
import { By } from '@angular/platform-browser';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { MatDialog } from '@angular/material/dialog';
import { of } from 'rxjs';

import { FieldSchema } from 'app/models/schema/fields';
import { FieldValue } from 'app/models/values/fields';
import { Message, t } from 'app/core/i18n/message';
import { TypedFixture } from 'app/core/testing/fixture';
import {
  COMPOSITE_DEFINITIONS,
  StubImagesService,
  field,
  filesChosen,
} from 'app/core/testing/fields';
import { ImagesService } from 'app/services/media/images.service';
import { CompositeFieldsService } from 'app/services/schema/composite-fields.service';
import { CompositeField } from 'app/shared/composite-field/composite-field';
import { ValueField } from 'app/shared/value-field/value-field';
import { ArrayField } from './array-field';

/**
 * The field above the array.
 *
 * An array's elements are edited by whatever the field above it renders, so the widget is handed
 * a template - and the template here is the same one `ValueField` hands it: this component
 * renders `ValueField` per element. That keeps the recursion in the spec, and it is what makes
 * "an array of composites that holds an array of itself" testable.
 */
@Component({
  imports: [ArrayField, ValueField],
  template: `
    <app-array-field
      [field]="field"
      [value]="value"
      [disabled]="disabled"
      [labelId]="labelId"
      [elementTemplate]="elementEditor"
      (valueChange)="setValue($event)"
      (errorChange)="errorChange.emit($event)"
    />
    <ng-template
      #elementEditor
      let-schema="schema"
      let-value="value"
      let-changed="changed"
      let-problem="problem"
    >
      <app-value-field
        [field]="schema"
        [value]="value"
        [disabled]="disabled"
        (valueChange)="changed($event)"
        (errorChange)="problem($event)"
      />
    </ng-template>
  `,
})
class ArrayHost {
  @ViewChild(ArrayField) array?: ArrayField;
  field!: FieldSchema;
  value: FieldValue = null;
  disabled = false;
  labelId = 'array-field-1';
  readonly valueChange = new EventEmitter<FieldValue>();
  readonly errorChange = new EventEmitter<Message | null>();

  /** What the editor above does with the widget's answer: it becomes the value it is given. */
  setValue(value: FieldValue) {
    this.value = value;
    this.valueChange.emit(value);
  }
}

describe('ArrayField', () => {
  let fixture: TypedFixture<ArrayHost>;
  let images: StubImagesService;

  beforeEach(async () => {
    images = new StubImagesService();
    await TestBed.configureTestingModule({
      imports: [ArrayHost],
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

  // A dialog lives in the document rather than in the fixture, so one left open is a `.thumb` the
  // next spec finds.
  afterEach(() => {
    TestBed.inject(MatDialog).closeAll();
  });

  /** Uses the host's inputs, so they are in place before the first change detection. */
  function create(fieldSchema: FieldSchema, value: FieldValue = null): ArrayField {
    fixture = TestBed.createComponent(ArrayHost);
    fixture.componentInstance.field = fieldSchema;
    fixture.componentInstance.value = value;
    fixture.detectChanges();
    return fixture.componentInstance.array as ArrayField;
  }

  function query(selector: string): Element | null {
    return fixture.nativeElement.querySelector(selector);
  }

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

  // An array that mixes references with something else is not all relations, so it stays on the
  // JSON view - and that view has to accept a reference object and name the item type.
  it('names and accepts a relation item type in an array that is not all relations', async () => {
    const component = create(
      field('related', {
        Array: [{ Relation: { target: { kind: 'collection', name: 'authors' } } }, 'Number'],
      }),
      [],
    );
    component.jsonMode.set(true);
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.nativeElement.textContent).toContain('Item types: Relation, Number');

    const problems: (Message | null)[] = [];
    component.errorChange.subscribe((problem) => problems.push(problem));
    const values: FieldValue[] = [];
    component.valueChange.subscribe((value) => values.push(value));

    component.onJsonChange('[{"target":"authors","item":1}, 2]');
    expect(problems[0]).toBeNull();
    expect(values.at(-1)).toEqual([{ target: 'authors', item: 1 }, 2]);
  });

  // The server tries each declared type in turn; this only rules out what none of them could
  // read, and it says so while the reader is looking at the box rather than after a save.
  it('refuses an item no declared item type could read', () => {
    const component = create(field('scores', { Array: ['Number'] }), []);
    const problems: (Message | null)[] = [];
    component.errorChange.subscribe((problem) => problems.push(problem));
    const values: FieldValue[] = [];
    component.valueChange.subscribe((value) => values.push(value));

    component.onJsonChange('[1, "two", 3]');

    expect(problems[0]).toEqual(
      t('content.arrayItemType', { field: 'scores[1]', types: 'Number' }),
    );
    // The last good value is kept, as it is for JSON that does not parse.
    expect(values).toEqual([]);
  });

  it('accepts what a declared item type could read, without second-guessing the server', () => {
    const tags = create(field('tags', { Array: [{ Text: {} }] }), []);
    const textProblems: (Message | null)[] = [];
    tags.errorChange.subscribe((problem) => textProblems.push(problem));
    tags.onJsonChange('["one", 2]');
    expect(textProblems[0]).not.toBeNull();

    // A date-shaped string is a string: an array of texts takes it.
    const dates = create(field('notes', { Array: [{ Text: {} }] }), []);
    const dateProblems: (Message | null)[] = [];
    dates.errorChange.subscribe((problem) => dateProblems.push(problem));
    const dateValues: FieldValue[] = [];
    dates.valueChange.subscribe((value) => dateValues.push(value));
    dates.onJsonChange('["2026-01-01"]');
    expect(dateProblems).toEqual([null]);
    expect(dateValues).toEqual([['2026-01-01']]);
  });

  it('seeds the JSON buffer from the incoming array', () => {
    const component = create(field('numbers', { Array: ['Number'] }), [1, 2]);
    expect(component.arrayText).toBe('[1,2]');
  });

  // The parent hands the emitted value straight back, and re-seeding from it would fight
  // whatever the user is typing.
  it('does not reset the buffer when its own value comes back', () => {
    const component = create(field('numbers', { Array: ['Number'] }), []);

    component.onJsonChange('[1, 2]');
    expect(component.arrayText).toBe('[1, 2]');

    fixture.componentInstance.value = component.value;
    fixture.detectChanges();

    expect(component.arrayText).toBe('[1, 2]');
  });

  it('reports invalid array JSON without emitting a value', () => {
    const component = create(field('numbers', { Array: ['Number'] }), []);
    const values: FieldValue[] = [];
    const errors: (Message | null)[] = [];
    component.valueChange.subscribe((value) => values.push(value));
    component.errorChange.subscribe((error) => errors.push(error));

    component.onJsonChange('[1, 2');
    expect(errors.at(-1)).toEqual(t('content.invalidJson', { field: 'numbers' }));
    // Nothing is emitted, so the previously valid value is what the parent still holds.
    expect(values).toHaveLength(0);

    component.onJsonChange('{"not":"an array"}');
    expect(errors.at(-1)).toEqual(t('content.expectedJsonArray', { field: 'numbers' }));

    component.onJsonChange('[1, 2]');
    expect(errors.at(-1)).toBeNull();
    expect(values).toEqual([[1, 2]]);

    component.onJsonChange('');
    expect(values.at(-1)).toEqual([]);
  });

  it('shows an image array as thumbnails, not as JSON', () => {
    const component = create(field('covers', { Array: ['Image'] }), [
      { id: 3, url: '/images/logo.png' },
    ]);

    expect(component.isImageArray()).toBe(true);
    const items = fixture.nativeElement.querySelectorAll<HTMLElement>('.array-item');
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

  it('adds several images from the library at once', async () => {
    const component = create(field('covers', { Array: ['Image'] }), []);
    const changes: FieldValue[] = [];
    component.valueChange.subscribe((value) => changes.push(value));

    (query('button.array-add') as HTMLButtonElement).click();
    await fixture.whenStable();

    // Multi mode: clicking ticks rather than choosing, until the add button is pressed.
    const thumbs = document.querySelectorAll<HTMLElement>('.thumb');
    thumbs[0].click();
    thumbs[1].click();
    await fixture.whenStable();

    // Tick marks are the picker's own state: what matters to the editor is only that ticking
    // has not chosen anything yet.
    expect(document.querySelectorAll('.thumb.selected').length).toBe(2);
    expect(changes).toEqual([]);

    (document.querySelector('button.array-add-selected') as HTMLButtonElement).click();
    await fixture.whenStable();
    fixture.detectChanges();

    expect(changes).toEqual([
      [
        { id: 3, url: '/images/logo.png' },
        { id: 4, url: '/images/photo.png' },
      ],
    ]);
    expect(fixture.nativeElement.querySelectorAll('.array-item').length).toBe(2);
    // Confirming is the whole gesture: the dialog is gone.
    expect(document.querySelector('.thumb')).toBeNull();
  });

  it('appends to the images an array already holds', async () => {
    const component = create(field('covers', { Array: ['Image'] }), [
      { id: 4, url: '/images/photo.png' },
    ]);

    (query('button.array-add') as HTMLButtonElement).click();
    await fixture.whenStable();
    (document.querySelectorAll('.thumb')[0] as HTMLButtonElement).click();
    await fixture.whenStable();
    const chosen: FieldValue[] = [];
    component.valueChange.subscribe((value) => chosen.push(value));
    (document.querySelector('button.array-add-selected') as HTMLButtonElement).click();
    await fixture.whenStable();
    fixture.detectChanges();

    expect(chosen.at(-1)).toEqual([
      { id: 4, url: '/images/photo.png' },
      { id: 3, url: '/images/logo.png' },
    ]);
  });

  it('reorders and removes images in an array', () => {
    const component = create(field('covers', { Array: ['Image'] }), [
      { id: 3, url: '/images/logo.png' },
      { id: 4, url: '/images/photo.png' },
    ]);
    const emitted: FieldValue[] = [];
    component.valueChange.subscribe((value) => emitted.push(value));

    (query('button[aria-label="move image 1 earlier"]') as HTMLButtonElement).click();
    expect(emitted.at(-1)).toEqual([
      { id: 4, url: '/images/photo.png' },
      { id: 3, url: '/images/logo.png' },
    ]);

    fixture.detectChanges();
    (query('button[aria-label="remove image 0"]') as HTMLButtonElement).click();
    expect(emitted.at(-1)).toEqual([{ id: 3, url: '/images/logo.png' }]);
  });

  it('keeps the JSON view in step with the thumbnails', async () => {
    const component = create(field('covers', { Array: ['Image'] }), []);

    (query('button.array-add') as HTMLButtonElement).click();
    await fixture.whenStable();
    (document.querySelectorAll('.thumb')[0] as HTMLButtonElement).click();
    await fixture.whenStable();
    (document.querySelector('button.array-add-selected') as HTMLButtonElement).click();
    await fixture.whenStable();
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

  it('edits a composite array element by element', () => {
    const component = create(field('blocks', { Array: [{ CompositeField: { id: 'seo' } }] }), [
      { id: 'seo', values: { description: 'first' } },
    ]);
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
    // The sub-values are the composite widget's, one level below the field that names the
    // definition.
    const composite = fixture.debugElement.queryAll(By.directive(CompositeField))[0]
      .componentInstance as CompositeField;
    expect(composite.compositeValues).toEqual({ description: 'first' });
  });

  it('adds an element with its definition defaults, and removes it again', () => {
    const component = create(field('blocks', { Array: [{ CompositeField: { id: 'seo' } }] }), []);
    const changes: FieldValue[] = [];
    component.valueChange.subscribe((value) => changes.push(value));

    component.addElement();
    expect(changes.at(-1)).toEqual([{ id: 'seo', values: { description: '' } }]);

    // The host hands an emission straight back as the value, as the editor above does, so this
    // is only the change detection that writes it into the elements on screen.
    const accept = () => fixture.detectChanges();

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
      ],
    );
    fixture.detectChanges();

    // One element per definition, each edited by that definition's schema.
    expect(component.elementItems.map((element) => element.id)).toEqual(['gallery', 'seo']);
    expect(component.elementSchemas[0].field_type).toEqual({ CompositeField: { id: 'gallery' } });

    const moved: FieldValue[] = [];
    component.valueChange.subscribe((value) => moved.push(value));
    component.moveElement(0, 1);

    expect(moved.at(-1)).toEqual([
      { id: 'seo', values: { description: 'second' } },
      { id: 'gallery', values: { images: [] } },
    ]);
  });

  it('stores what an element editor emits under the definition the element names', () => {
    const component = create(field('blocks', { Array: [{ CompositeField: { id: 'seo' } }] }), []);
    const changes: FieldValue[] = [];
    component.valueChange.subscribe((value) => changes.push(value));

    component.addElement();
    component.setElementValues(0, { description: 'typed' });

    expect(changes.at(-1)).toEqual([{ id: 'seo', values: { description: 'typed' } }]);
  });

  it('stops where the value stops when a definition holds an array of itself', () => {
    // The editor draws array elements from the value and composite sub-fields from the schema, so
    // this is where a self-referencing definition ends: one level per stored element, and an
    // element whose own array is empty draws nothing. (That is also what makes the schema legal.)
    create(field('blocks', { Array: [{ CompositeField: { id: 'tree' } }] }), [
      { id: 'tree', values: { line: 'root', children: [] } },
    ]);
    fixture.detectChanges();

    expect(fixture.nativeElement.querySelectorAll('.composite-element').length).toBe(1);
    expect(
      fixture.nativeElement.querySelectorAll('.composite-element .composite-element').length,
    ).toBe(0);
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
    ]);
    fixture.detectChanges();

    expect(fixture.nativeElement.querySelectorAll('.composite-element').length).toBe(2);
    expect(
      fixture.nativeElement.querySelectorAll('.composite-element .composite-element').length,
    ).toBe(1);
  });

  it('adds to an array that lives inside an element', () => {
    // An element holds an array of the same definition, and both have an "Add element" button:
    // the nested one is inside the element, so it comes first in the document.
    const component = create(field('blocks', { Array: [{ CompositeField: { id: 'tree' } }] }), [
      { id: 'tree', values: { line: 'root', children: [] } },
    ]);
    fixture.detectChanges();
    const emitted: FieldValue[] = [];
    component.valueChange.subscribe((value) => emitted.push(value));

    const addButtons =
      fixture.nativeElement.querySelectorAll<HTMLButtonElement>('button.array-add');
    expect(addButtons.length).toBe(2);
    addButtons[0].click();
    fixture.detectChanges();

    expect(fixture.nativeElement.querySelectorAll('.composite-element').length).toBe(2);
    expect(
      fixture.nativeElement.querySelectorAll('.composite-element .composite-element').length,
    ).toBe(1);
    expect((emitted.at(-1) as { values?: unknown }[])[0].values).toEqual({
      line: 'root',
      children: [{ id: 'tree', values: { line: '', children: [] } }],
    });
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
