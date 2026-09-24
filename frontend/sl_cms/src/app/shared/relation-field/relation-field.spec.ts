import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { Message, t } from 'app/core/i18n/message';
import { FieldSchema, FieldType } from 'app/models/schema/fields';
import { FieldValue } from 'app/models/values/fields';
import { TypedFixture } from 'app/core/testing/fixture';
import { RelationField } from './relation-field';

function field(
  name: string,
  field_type: FieldType,
  layout: Partial<FieldSchema> = {},
): FieldSchema {
  return { name, field_type, required: false, width: 12, height: 1, ...layout };
}

describe('RelationField', () => {
  let fixture: TypedFixture<RelationField>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [RelationField],
      providers: [provideHttpClient(), provideHttpClientTesting()],
    }).compileComponents();
  });

  /** Uses `setInput` so the inputs are in place before the first change detection. */
  function create(fieldSchema: FieldSchema, value: FieldValue = null): RelationField {
    fixture = TestBed.createComponent(RelationField);
    fixture.componentRef.setInput('field', fieldSchema);
    fixture.componentRef.setInput('value', value);
    fixture.detectChanges();
    return fixture.componentInstance;
  }

  // A relation is a set of references, so it is array-shaped too - but what an element may hold
  // comes from the field's own target, not from array item types.
  it('edits a relation as the references its target accepts', () => {
    const component = create(
      field('authors', { Relation: { target: { kind: 'collection', name: 'authors' } } }),
      [{ target: 'authors', item: 1 }],
    );
    const values: FieldValue[] = [];
    const errors: (Message | null)[] = [];
    component.valueChange.subscribe((value) => values.push(value));
    component.errorChange.subscribe((error) => errors.push(error));

    // Seeded from the value the server sent, so a load-then-save keeps what was there.
    expect(component.arrayText).toBe('[{"target":"authors","item":1}]');

    component.onJsonChange('[{"target":"categories","item":1}]');
    expect(errors.at(-1)).toEqual(
      t('content.relationTargetMismatch', { field: 'authors[0]', target: 'authors' }),
    );
    expect(values).toHaveLength(0);

    component.onJsonChange('[{"target":"authors"}]');
    expect(errors.at(-1)).toEqual(t('content.relationItemId', { field: 'authors[0]' }));

    component.onJsonChange('[{"item":1}]');
    expect(errors.at(-1)).toEqual(
      t('content.relationTargetMismatch', { field: 'authors[0]', target: 'authors' }),
    );

    component.onJsonChange('["authors"]');
    expect(errors.at(-1)).toEqual(t('content.relationShape', { field: 'authors[0]' }));

    component.onJsonChange('[{"target":"authors","item":2}]');
    expect(errors.at(-1)).toBeNull();
    expect(values).toEqual([[{ target: 'authors', item: 2 }]]);
  });

  it('holds one reference unless the field asks for several', () => {
    const single = create(
      field('author', {
        Relation: { target: { kind: 'collection', name: 'authors' }, has_many: false },
      }),
    );
    const singleErrors: (Message | null)[] = [];
    single.errorChange.subscribe((error) => singleErrors.push(error));
    const singleValues: FieldValue[] = [];
    single.valueChange.subscribe((value) => singleValues.push(value));

    single.onJsonChange('[{"target":"authors","item":1},{"target":"authors","item":2}]');
    expect(singleErrors.at(-1)).toEqual(t('content.relationSingle', { field: 'author' }));
    // Nothing is emitted, so the parent keeps the value it had.
    expect(singleValues).toEqual([]);

    const many = create(
      field('authors', {
        Relation: { target: { kind: 'collection', name: 'authors' }, has_many: true },
      }),
    );
    const manyValues: FieldValue[] = [];
    many.valueChange.subscribe((value) => manyValues.push(value));
    many.onJsonChange('[{"target":"authors","item":1},{"target":"authors","item":2}]');
    expect(manyValues).toEqual([
      [
        { target: 'authors', item: 1 },
        { target: 'authors', item: 2 },
      ],
    ]);
  });

  // The box edits the references, and the screen says which items they are: an id says nothing to
  // a reader, and the target's schema is what names an item.
  it('says what the references it holds are called', async () => {
    const http = TestBed.inject(HttpTestingController);
    const component = create(
      field('category', { Relation: { target: { kind: 'collection', name: 'categories' } } }),
      [
        { target: 'categories', item: 3 },
        { target: 'categories', item: 9 },
      ],
    );
    await fixture.whenStable();

    http
      .expectOne((request) => request.url === '/api/models/collections/categories/items/titles')
      .flush({ 3: '技術' });
    await fixture.whenStable();
    fixture.detectChanges();

    // The chips are the value, and they read as the item: the reference the target cannot name
    // keeps the reference, which is all that is knowable about it.
    const chips = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('mat-chip-row .reference-label'),
      (chip: HTMLElement) => chip.textContent?.trim(),
    );
    expect(chips).toEqual(['技術', 'categories #9']);

    // The JSON box says the names too, for an author working in it.
    component.jsonMode.set(true);
    fixture.detectChanges();
    expect(component.referenceNames()).toEqual(['技術']);
    expect(fixture.nativeElement.textContent).toContain('References: 技術');
  });

  // A relation is a list now: the order is what a site shows, so it can be changed from the chips
  // and the change is a change to the value.
  it('moves a reference, and only when there is more than one', () => {
    const many = create(
      field('authors', {
        Relation: { target: { kind: 'collection', name: 'authors' }, has_many: true },
      }),
      [
        { target: 'authors', item: 1 },
        { target: 'authors', item: 2 },
        { target: 'authors', item: 3 },
      ],
    );
    const emitted: FieldValue[] = [];
    many.valueChange.subscribe((value) => emitted.push(value));

    many.moveReference(2, -1);
    expect(emitted.at(-1)).toEqual([
      { target: 'authors', item: 1 },
      { target: 'authors', item: 3 },
      { target: 'authors', item: 2 },
    ]);
    // Off the end is nothing to do, not a wrap-around.
    many.moveReference(0, -1);
    expect(emitted).toHaveLength(1);

    // One reference has no order to change, so the controls are not offered.
    create(
      field('author', {
        Relation: { target: { kind: 'collection', name: 'authors' }, has_many: false },
      }),
      [{ target: 'authors', item: 1 }],
    );
    expect(fixture.nativeElement.querySelector('button[aria-label*="move reference"]')).toBeFalsy();
  });

  it('names a page reference without an item id', () => {
    const component = create(
      field('landing', { Relation: { target: { kind: 'single_page', name: 'home' } } }),
      [{ target: 'home' }],
    );
    const errors: (Message | null)[] = [];
    component.errorChange.subscribe((error) => errors.push(error));
    const values: FieldValue[] = [];
    component.valueChange.subscribe((value) => values.push(value));

    expect(component.arrayText).toBe('[{"target":"home"}]');
    expect(component.relationHint()?.key).toBe('content.relationJsonHintPage');

    // A page has no id, so one sent anyway is a value the schema has nowhere to keep.
    component.onJsonChange('[{"target":"home","item":1}]');
    expect(errors.at(-1)).toEqual(t('content.relationPageHasNoItem', { field: 'landing[0]' }));

    component.onJsonChange('[{"target":"home"}]');
    expect(errors.at(-1)).toBeNull();
    expect(values).toEqual([[{ target: 'home' }]]);
  });
});
