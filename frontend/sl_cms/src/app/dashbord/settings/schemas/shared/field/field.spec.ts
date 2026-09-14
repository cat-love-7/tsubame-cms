import { ComponentFixture, TestBed } from '@angular/core/testing';

import { DefaultFieldLayout, FieldDefaults, FieldSchema } from 'app/models/schema/fields';

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
  let fixture: ComponentFixture<Field>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [Field]
    })
    .compileComponents();

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

    component.onArrayItemTypesChange(['Number', 'Image']);

    expect(component.field.field_type).toEqual({ Array: ['Image'] });
  });

  it('drops Image when Number is the one just added', () => {
    component.field = field({ field_type: { Array: ['Image'] } });

    component.onArrayItemTypesChange(['Image', 'Number']);

    expect(component.field.field_type).toEqual({ Array: ['Number'] });
  });

  it('leaves the item types of a field that is not an array alone', () => {
    component.field = field({ field_type: 'Number' });

    component.onArrayItemTypesChange(['Image']);

    expect(component.field.field_type).toBe('Number');
  });

  it('renders the editor with its labels from the catalogs', async () => {
    fixture.componentRef.setInput('field', field());
    await fixture.whenStable();
    fixture.detectChanges();

    const labels = Array.from(
      fixture.nativeElement.querySelectorAll('mat-label'),
      (label: HTMLElement) => label.textContent?.trim(),
    );
    expect(labels).toContain('Field Name');
    expect(labels).toContain('Field Type');

    // The width presets: fractions need no translation, the one word does.
    const presets = Array.from(
      fixture.nativeElement.querySelectorAll('.width-presets button'),
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

  it('brings the enum editor in when the field is an enum', async () => {
    fixture.componentRef.setInput('field', field({ field_type: { TextEnum: ['draft'] } }));
    await fixture.whenStable();
    fixture.detectChanges();

    const enumEditor = fixture.nativeElement.querySelector('app-enum-field') as HTMLElement;
    expect(enumEditor).toBeTruthy();
    expect(enumEditor.textContent).toContain('draft');
  });
});
