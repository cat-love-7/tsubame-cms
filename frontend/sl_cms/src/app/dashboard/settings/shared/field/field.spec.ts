import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { HttpTestingController } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import {
  DefaultFieldLayout,
  FieldDefaults,
  FieldSchema,
  newFieldType,
} from 'app/models/schema/fields';

import { TypedFixture } from 'app/core/testing/fixture';
import { Field } from './field';

/** A field as the schema editor holds it: a name, a type, and a place on the grid. */
function field(overrides: Partial<FieldSchema> = {}): FieldSchema {
  return {
    name: 'title',
    field_type: FieldDefaults.Text,
    required: false,
    ...DefaultFieldLayout,
    ...overrides,
  };
}

describe('Field', () => {
  let component: Field;
  let fixture: TypedFixture<Field>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [Field],
      // An array field asks for the composite definitions the item types may name.
      providers: [provideHttpClient(), provideHttpClientTesting()],
    }).compileComponents();

    fixture = TestBed.createComponent(Field);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  it('switches the type to that type own default, and reports the change', () => {
    const emitted: FieldSchema[] = [];
    component.fieldChange.subscribe((value) => emitted.push(value));

    component.onFieldTypeChange('TextEnum');
    expect(component.field.field_type).toEqual({ TextEnum: [] });

    component.onFieldTypeChange('Array');
    // An array starts empty rather than with a type nobody chose.
    expect(component.field.field_type).toEqual({ Array: [] });

    component.onFieldTypeChange('Number');
    expect(component.field.field_type).toBe('Number');

    expect(emitted.length).toBe(3);
  });

  it('takes the width from a preset, and reports it', () => {
    const emitted: FieldSchema[] = [];
    component.fieldChange.subscribe((value) => emitted.push(value));

    component.setWidth(6);

    expect(component.field.width).toBe(6);
    expect(emitted).toEqual([component.field]);
  });

  it('drops Number when Image is added to an array, which has no type tags', () => {
    // The wire carries bare values, so an image id and a number cannot be told apart.
    component.field = field({ field_type: { Array: ['Number'] } });

    component.onScalarItemTypesChange(['Number', 'Image']);

    expect(component.field.field_type).toEqual({ Array: ['Image'] });
  });

  it('drops Image when Number is the one just added', () => {
    component.field = field({ field_type: { Array: ['Image'] } });

    component.onScalarItemTypesChange(['Image', 'Number']);

    expect(component.field.field_type).toEqual({ Array: ['Number'] });
  });

  it('leaves the item types of a field that is not an array alone', () => {
    component.field = field({ field_type: 'Number' });

    component.onScalarItemTypesChange(['Image']);

    expect(component.field.field_type).toBe('Number');
  });

  it('renders the editor with its labels from the catalogs', async () => {
    fixture.componentRef.setInput('field', field());
    await fixture.whenStable();
    fixture.detectChanges();

    const labels = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('mat-label'),
      (label: HTMLElement) => label.textContent?.trim(),
    );
    expect(labels).toContain('Field Name');
    expect(labels).toContain('Field Type');

    // The width presets: fractions need no translation, the one word does.
    const presets = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('.width-presets button'),
      (button: HTMLElement) => button.textContent?.trim(),
    );
    expect(presets).toContain('1/2');
    expect(presets).toContain('Full');

    // An array field is the only one that asks for item types.
    expect(fixture.nativeElement.querySelector('mat-select[name="arrayItemTypes"]')).toBeNull();

    fixture.componentRef.setInput('field', field({ field_type: { Array: ['Image'] } }));
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('mat-select[name="arrayItemTypes"]')).toBeTruthy();
  });

  it('offers the composite definitions as array item types', async () => {
    const http = TestBed.inject(HttpTestingController);
    fixture.componentRef.setInput('field', field({ field_type: { Array: ['Number'] } }));
    await fixture.whenStable();
    fixture.detectChanges();

    // The list is only asked for once an array is being edited.
    http.expectOne('/api/models/composite_fields').flush({ seo: [], gallery: [] });
    fixture.detectChanges();

    const compositeControl = fixture.nativeElement.querySelector(
      'mat-select[name="arrayCompositeTypes"]',
    );
    expect(compositeControl).toBeTruthy();

    component.onCompositeItemTypesChange(['gallery']);
    expect(component.field.field_type).toEqual({
      Array: ['Number', { CompositeField: { id: 'gallery' } }],
    });

    // Changing the scalars must not drop the composite the user chose in the other control...
    component.onScalarItemTypesChange(['Number', 'Boolean']);
    expect(component.field.field_type).toEqual({
      Array: ['Number', 'Boolean', { CompositeField: { id: 'gallery' } }],
    });

    // ...and changing the composites must not drop the scalars.
    component.onCompositeItemTypesChange(['seo', 'gallery']);
    expect(component.field.field_type).toEqual({
      Array: [
        'Number',
        'Boolean',
        { CompositeField: { id: 'seo' } },
        { CompositeField: { id: 'gallery' } },
      ],
    });
  });

  it('does not ask for composite definitions unless the field is an array', async () => {
    const http = TestBed.inject(HttpTestingController);
    fixture.componentRef.setInput('field', field({ field_type: { Text: {} } }));
    await fixture.whenStable();

    http.expectNone('/api/models/composite_fields');
  });

  it('offers the unique flag for a text field, and only where it means something', async () => {
    fixture.componentRef.setInput('field', field());
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('input[name="fieldUnique"]')).toBeTruthy();

    // The server refuses it for a type whose values are not compared as text.
    fixture.componentRef.setInput('field', field({ field_type: 'Number' }));
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('input[name="fieldUnique"]')).toBeNull();

    // ...and for a screen with nothing to compare a value against (a single page, a composite).
    fixture.componentRef.setInput('uniqueAllowed', false);
    fixture.componentRef.setInput('field', field());
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('input[name="fieldUnique"]')).toBeNull();
  });

  // A reference names the item it points at, and which field does the naming is the schema's call -
  // but only where a reference can point: a collection's items and a page, not an embedded block.
  it('offers the title only where a reference can point', async () => {
    const text = field({ name: 'name', is_title: false });
    fixture.componentRef.setInput('field', text);
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('input[name="fieldIsTitle"]')).toBeNull();

    fixture.componentRef.setInput('titleAllowed', true);
    await fixture.whenStable();
    fixture.detectChanges();
    const box = fixture.nativeElement.querySelector(
      'input[name="fieldIsTitle"]',
    ) as HTMLInputElement;
    expect(box).toBeTruthy();
    expect(box.checked).toBe(false);

    box.click();
    fixture.detectChanges();
    expect(text.is_title).toBe(true);
  });

  // Only a collection has a list of its items, so only its schema is offered the column setting.
  it('offers the list column only where there is a list', async () => {
    const text = field({ name: 'title', show_in_list: false });
    fixture.componentRef.setInput('field', text);
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('input[name="fieldInList"]')).toBeNull();

    fixture.componentRef.setInput('listAllowed', true);
    await fixture.whenStable();
    fixture.detectChanges();
    const box = fixture.nativeElement.querySelector(
      'input[name="fieldInList"]',
    ) as HTMLInputElement;
    expect(box).toBeTruthy();
    expect(box.checked).toBe(false);

    // Ticking it is what puts the field in the list, and it travels with the field.
    box.click();
    fixture.detectChanges();
    expect(text.show_in_list).toBe(true);
  });

  // A slug's only option is where an editor may take the value from; the rule and the length cap
  // belong to the type, so there is nothing else to configure.
  it('offers the sibling text fields as a slug\u2019s source', async () => {
    const address = field({ name: 'address', field_type: { Slug: {} } });
    fixture.componentRef.setInput('field', address);
    fixture.componentRef.setInput('siblingFields', [
      field({ name: 'title' }),
      field({ name: 'body', field_type: { Markdown: {} } }),
      field({ name: 'count', field_type: 'Number' }),
      address,
    ]);
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.componentInstance.isSlug()).toBe(true);
    expect(
      fixture.componentInstance.slugSourceOptions().map((candidate) => candidate.name),
    ).toEqual(['title', 'body']);

    fixture.componentInstance.setGenerateFrom('title');
    expect(address.field_type).toEqual({ Slug: { generate_from: 'title' } });

    // Choosing "none" leaves the option out entirely, rather than naming an empty field.
    fixture.componentInstance.setGenerateFrom('');
    expect(address.field_type).toEqual({ Slug: {} });
  });

  it('offers no unique flag for a slug, which is unique by being one', async () => {
    fixture.componentRef.setInput('field', field({ name: 'address', field_type: { Slug: {} } }));
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.nativeElement.querySelector('input[name="fieldUnique"]')).toBeNull();
    expect(fixture.nativeElement.querySelector('mat-select[name="slugGenerateFrom"]')).toBeTruthy();
  });

  it('brings the enum editor in when the field is an enum', async () => {
    fixture.componentRef.setInput('field', field({ field_type: { TextEnum: ['draft'] } }));
    await fixture.whenStable();
    fixture.detectChanges();

    const enumEditor = fixture.nativeElement.querySelector('app-enum-field') as HTMLElement;
    expect(enumEditor).toBeTruthy();
    expect(enumEditor.textContent).toContain('draft');
  });

  // A relation needs both lists: it can point at any collection or any single page.
  it('offers the collections and single pages as relation targets', async () => {
    const http = TestBed.inject(HttpTestingController);
    fixture.componentRef.setInput('field', field({ field_type: newFieldType('Relation') }));
    await fixture.whenStable();
    fixture.detectChanges();

    // Asked for only once a relation is being edited.
    http.expectOne('/api/models/collections').flush(['authors', 'categories']);
    http.expectOne('/api/models/single_pages').flush(['home']);
    fixture.detectChanges();

    expect(component.relationTargetKind()).toBe('collection');
    expect(component.relationTargetNames()).toEqual(['authors', 'categories']);
    expect(fixture.nativeElement.querySelector('mat-select[name="relationTarget"]')).toBeTruthy();

    component.setRelationTargetName('authors');
    expect(component.field.field_type).toEqual({
      Relation: { target: { kind: 'collection', name: 'authors' }, has_many: false },
    });

    component.relationHasMany = true;
    expect(component.relationHasMany).toBe(true);

    component.setRelationInverseName('articles');
    expect(component.relationInverseName()).toBe('articles');

    // A page and a collection may share a name, so switching clears it rather than pointing the
    // relation somewhere else by accident. The inverse label names the direction, not the target,
    // so it is left for the user to keep or change.
    component.setRelationTargetKind('single_page');
    expect(component.field.field_type).toEqual({
      Relation: {
        target: { kind: 'single_page', name: '' },
        has_many: false,
        inverse_name: 'articles',
      },
    });
    expect(component.relationTargetNames()).toEqual(['home']);

    // A page is one item, so "several" is not on offer there.
    component.relationHasMany = true;
    expect(component.relationHasMany).toBe(false);
  });

  // The controls have to work by clicking them, not only by calling the method behind them.
  it('takes several references from the checkbox', async () => {
    const http = TestBed.inject(HttpTestingController);
    fixture.componentRef.setInput('field', field({ field_type: newFieldType('Relation') }));
    await fixture.whenStable();
    http.expectOne('/api/models/collections').flush(['authors']);
    http.expectOne('/api/models/single_pages').flush([]);
    fixture.detectChanges();

    component.setRelationTargetName('authors');
    fixture.detectChanges();

    const checkbox = fixture.nativeElement.querySelector(
      'input[name="relationHasMany"]',
    ) as HTMLInputElement;
    expect(checkbox).toBeTruthy();
    checkbox.click();
    fixture.detectChanges();

    expect(component.field.field_type).toEqual({
      Relation: { target: { kind: 'collection', name: 'authors' }, has_many: true },
    });
  });

  it('does not ask for relation targets unless the field is a relation', async () => {
    const http = TestBed.inject(HttpTestingController);
    fixture.componentRef.setInput('field', field({ field_type: { Text: {} } }));
    await fixture.whenStable();

    http.expectNone('/api/models/collections');
    http.expectNone('/api/models/single_pages');
    expect(fixture.nativeElement.querySelector('mat-select[name="relationTarget"]')).toBeNull();
  });

  // Switching type must not hand the field the shared default object: the widgets write into it.
  it('gives the field a copy of the type it switches to', () => {
    const fresh: TypedFixture<Field> = TestBed.createComponent(Field);
    fresh.componentInstance.field = {
      name: 'title',
      field_type: 'Number',
      required: false,
      width: 12,
      height: 1,
    };

    fresh.componentInstance.onFieldTypeChange('Text');
    const first = fresh.componentInstance.field.field_type as { Text: { max_length?: number } };
    first.Text.max_length = 30;

    fresh.componentInstance.onFieldTypeChange('Markdown');
    fresh.componentInstance.onFieldTypeChange('Text');
    const again = fresh.componentInstance.field.field_type as { Text: { max_length?: number } };

    expect(again.Text.max_length).toBeUndefined();
  });
});
