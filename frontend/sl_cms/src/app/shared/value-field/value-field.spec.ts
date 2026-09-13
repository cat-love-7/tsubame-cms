import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { FieldSchema, FieldType } from 'app/models/schema/fields';
import { FieldValue } from 'app/models/values/fields';
import { ValueField } from './value-field';

function field(name: string, field_type: FieldType): FieldSchema {
  return { name, field_type, required: false, width: 12, height: 1 };
}

describe('ValueField', () => {
  let fixture: ComponentFixture<ValueField>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [ValueField],
      providers: [provideHttpClient(), provideHttpClientTesting()],
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

  it('explains that composite fields are not editable rather than dropping them', () => {
    create(field('seo', { CompositeField: { id: 'seo' } }));
    expect(fixture.nativeElement.textContent).toContain('not editable here yet');
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
});
