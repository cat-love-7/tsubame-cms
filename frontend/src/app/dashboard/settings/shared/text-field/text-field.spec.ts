import { TestBed } from '@angular/core/testing';

import { TypedFixture } from 'app/core/testing/fixture';

import { TextField } from './text-field';

describe('TextField', () => {
  let component: TextField;
  let fixture: TypedFixture<TextField>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [TextField],
    }).compileComponents();

    fixture = TestBed.createComponent(TextField);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  // Only a plain text field can be one line or a box; a Markdown field is a box by nature, so the
  // choice is not offered for it (the field editor says which it is drawing).
  it('offers the multi-line choice for a text field, and not for Markdown', async () => {
    fixture.componentRef.setInput('multilineAllowed', false);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('mat-checkbox')).toBeNull();

    fixture.componentRef.setInput('multilineAllowed', true);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('mat-checkbox')).toBeTruthy();
  });

  // The option is part of the field's definition, so it travels back the way the lengths do.
  it('carries the multi-line choice into the schema', async () => {
    fixture.componentRef.setInput('multilineAllowed', true);
    fixture.componentRef.setInput('fieldOptions', {});
    await fixture.whenStable();
    const emitted: unknown[] = [];
    component.fieldOptionsChange.subscribe((options) => emitted.push(options));

    component.fieldOptions.multiline = true;
    component.fieldOptionsChange.emit(component.fieldOptions);

    expect(emitted).toEqual([{ multiline: true }]);
  });
});
