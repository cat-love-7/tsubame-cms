import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MatChipEditedEvent, MatChipInputEvent } from '@angular/material/chips';

import { EnumField } from './enum-field';

/** What the chip input hands `addEnumOption`. */
function typed(value: string): MatChipInputEvent {
  return { value, chipInput: { clear: () => {} } } as MatChipInputEvent;
}

/** What a chip edit hands `editEnumOption`. */
function edited(value: string): MatChipEditedEvent {
  return { value } as MatChipEditedEvent;
}

describe('EnumField', () => {
  let component: EnumField;
  let fixture: ComponentFixture<EnumField>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [EnumField],
    }).compileComponents();

    fixture = TestBed.createComponent(EnumField);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  // The options travel as a JSON array on the wire, and they used to live in a `Set`: the
  // in-place mutation notified nobody, so an edit was lost. Each of these has to come back as
  // a new array.

  it('adds a typed value, trimmed', () => {
    component.addEnumOption(typed('  draft  '));
    expect(component.field()).toEqual(['draft']);
  });

  it('ignores an empty value and one that is already there', () => {
    component.field.set(['draft']);

    component.addEnumOption(typed('   '));
    component.addEnumOption(typed('draft'));

    expect(component.field()).toEqual(['draft']);
  });

  it('removes one value and leaves the others', () => {
    component.field.set(['draft', 'published']);

    component.removeEnumOption('draft');

    expect(component.field()).toEqual(['published']);
  });

  it('renames a value, and removes it when the new name is blank', () => {
    component.field.set(['draft', 'published']);

    component.editEnumOption('draft', edited('  in review '));
    expect(component.field()).toEqual(['in review', 'published']);

    component.editEnumOption('in review', edited('   '));
    expect(component.field()).toEqual(['published']);
  });

  it('shows one chip per value, each named in the message for it', async () => {
    // The labels are catalog messages with the value filled in. A chip whose message lost the
    // placeholder would tell a screen reader to press "{{value}} を削除".
    component.field.set(['draft', 'published']);
    await fixture.whenStable();
    fixture.detectChanges();

    const chips = fixture.nativeElement.querySelectorAll('mat-chip-row') as NodeListOf<HTMLElement>;
    expect(chips.length).toBe(2);
    expect(chips[0].textContent).toContain('draft');

    const remove = chips[0].querySelector('button[matChipRemove]') as HTMLButtonElement;
    expect(remove.getAttribute('aria-label')).toContain('draft');
    expect(remove.getAttribute('aria-label')).not.toContain('{{');

    const edit = chips[0].querySelector('button[matChipEdit]') as HTMLButtonElement;
    expect(edit.getAttribute('aria-label')).toContain('draft');
  });
});
