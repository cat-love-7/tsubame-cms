import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { Message, t } from 'app/core/i18n/message';
import { FieldSchema, FieldType, RelationTarget } from 'app/models/schema/fields';
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

const AUTHORS: RelationTarget = { kind: 'collection', name: 'authors' };
const CATEGORIES: RelationTarget = { kind: 'collection', name: 'categories' };
const HOME: RelationTarget = { kind: 'single_page', name: 'home' };

describe('RelationField', () => {
  let fixture: TypedFixture<RelationField>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [RelationField],
      providers: [provideHttpClient(), provideHttpClientTesting()],
    }).compileComponents();
  });

  /** Uses `setInput` so the inputs are in place before the first change detection. */
  function create(
    fieldSchema: FieldSchema,
    value: FieldValue = null,
    targets: RelationTarget[] = [AUTHORS],
    multiple = false,
  ): RelationField {
    fixture = TestBed.createComponent(RelationField);
    fixture.componentRef.setInput('field', fieldSchema);
    fixture.componentRef.setInput('targets', targets);
    fixture.componentRef.setInput('multiple', multiple);
    fixture.componentRef.setInput('value', value);
    fixture.detectChanges();
    return fixture.componentInstance;
  }

  // One relation is one object or null, and the box is checked against the target the field names.
  it('edits one relation as a single reference', () => {
    const component = create(field('author', { Relation: { target: AUTHORS } }), {
      target: 'authors',
      item: 1,
    });
    const values: FieldValue[] = [];
    const errors: (Message | null)[] = [];
    component.valueChange.subscribe((value) => values.push(value));
    component.errorChange.subscribe((error) => errors.push(error));

    expect(component.relationIsSingle()).toBe(true);
    // Seeded from the value the server sent, so a load-then-save keeps what was there.
    expect(component.arrayText).toBe('{"target":"authors","item":1}');
    expect(component.relationHint()?.key).toBe('content.relationJsonHintOne');

    component.onJsonChange('[{"target":"authors","item":2}]');
    expect(errors.at(-1)).toEqual(t('content.relationExpectedOne', { field: 'author' }));
    expect(values).toHaveLength(0);

    component.onJsonChange('{"target":"categories","item":1}');
    expect(errors.at(-1)).toEqual(
      t('content.relationTargetMismatch', { field: 'author[0]', targets: 'authors' }),
    );

    component.onJsonChange('{"target":"authors"}');
    expect(errors.at(-1)).toEqual(t('content.relationItemId', { field: 'author[0]' }));

    component.onJsonChange('{"target":"authors","item":2}');
    expect(errors.at(-1)).toBeNull();
    expect(values).toEqual([{ target: 'authors', item: 2 }]);

    component.onJsonChange('null');
    expect(errors.at(-1)).toBeNull();
    expect(values.at(-1)).toBeNull();
  });

  it('names a page reference without an item id, and only ever holds one', () => {
    const component = create(field('landing', { Relation: { target: HOME } }), null, [HOME]);
    const errors: (Message | null)[] = [];
    component.errorChange.subscribe((error) => errors.push(error));
    const values: FieldValue[] = [];
    component.valueChange.subscribe((value) => values.push(value));

    expect(component.arrayText).toBe('null');
    expect(component.relationHint()?.key).toBe('content.relationJsonHintPage');

    // A page has no id, so one sent anyway is a value the schema has nowhere to keep.
    component.onJsonChange('{"target":"home","item":1}');
    expect(errors.at(-1)).toEqual(t('content.relationPageHasNoItem', { field: 'landing[0]' }));

    component.onJsonChange('{"target":"home"}');
    expect(errors.at(-1)).toBeNull();
    expect(values).toEqual([{ target: 'home' }]);
  });

  // A pick replaces what a single relation held rather than refusing it, and the arrows that order
  // a list are not offered.
  it('replaces the one reference a single relation holds', () => {
    const component = create(field('author', { Relation: { target: AUTHORS } }), {
      target: 'authors',
      item: 1,
    });
    const values: FieldValue[] = [];
    component.valueChange.subscribe((value) => values.push(value));

    component.toggleReference({ target: 'authors', item: 2 });
    expect(values.at(-1)).toEqual({ target: 'authors', item: 2 });

    expect(fixture.nativeElement.querySelector('button[aria-label*="move reference"]')).toBeFalsy();
  });

  // Several references are an array of relations: a list, so it grows, has an order, and can name
  // several targets (one item type each).
  it('edits several references as a list, with an order', () => {
    const many = create(
      field('related', { Array: [{ Relation: { target: AUTHORS } }] }),
      [
        { target: 'authors', item: 1 },
        { target: 'authors', item: 2 },
        { target: 'authors', item: 3 },
      ],
      [AUTHORS],
      true,
    );
    const emitted: FieldValue[] = [];
    many.valueChange.subscribe((value) => emitted.push(value));

    expect(many.relationIsSingle()).toBe(false);

    many.moveReference(2, -1);
    expect(emitted.at(-1)).toEqual([
      { target: 'authors', item: 1 },
      { target: 'authors', item: 3 },
      { target: 'authors', item: 2 },
    ]);
    // Off the end is nothing to do, not a wrap-around.
    many.moveReference(0, -1);
    expect(emitted).toHaveLength(1);

    // The picker appends, and clicking what is picked takes it away again.
    many.toggleReference({ target: 'authors', item: 4 });
    expect(emitted.at(-1)).toEqual([
      { target: 'authors', item: 1 },
      { target: 'authors', item: 3 },
      { target: 'authors', item: 2 },
      { target: 'authors', item: 4 },
    ]);
  });

  it('accepts any target an array declares, and rejects one it does not', () => {
    const component = create(
      field('related', {
        Array: [{ Relation: { target: AUTHORS } }, { Relation: { target: HOME } }],
      }),
      [],
      [AUTHORS, HOME],
      true,
    );
    const errors: (Message | null)[] = [];
    component.errorChange.subscribe((error) => errors.push(error));
    const values: FieldValue[] = [];
    component.valueChange.subscribe((value) => values.push(value));

    expect(component.relationHint()?.params).toEqual({ targets: 'authors, home' });

    component.onJsonChange('[{"target":"authors","item":1},{"target":"home"}]');
    expect(errors.at(-1)).toBeNull();
    expect(values.at(-1)).toEqual([{ target: 'authors', item: 1 }, { target: 'home' }]);

    component.onJsonChange('[{"target":"categories","item":1}]');
    expect(errors.at(-1)).toEqual(
      t('content.relationTargetMismatch', { field: 'related[0]', targets: 'authors, home' }),
    );

    // An array of references takes an array, not one object.
    component.onJsonChange('{"target":"authors","item":1}');
    expect(errors.at(-1)).toEqual(t('content.expectedJsonArray', { field: 'related' }));
  });

  // The box edits the references, and the screen says which items they are: an id says nothing to
  // a reader, and the target's schema is what names an item.
  it('says what the references it holds are called', async () => {
    const http = TestBed.inject(HttpTestingController);
    const component = create(
      field('category', { Relation: { target: CATEGORIES } }),
      [
        { target: 'categories', item: 3 },
        { target: 'categories', item: 9 },
      ],
      [CATEGORIES],
      true,
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

  it('takes the references it holds from the single value it was given', () => {
    const component = create(field('author', { Relation: { target: AUTHORS } }), {
      target: 'authors',
      item: 1,
    });

    expect(component.relationRefs()).toEqual([{ target: 'authors', item: 1 }]);
  });
});
