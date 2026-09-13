import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { provideRouter } from '@angular/router';

import { Edit } from './edit';

describe('Edit', () => {
  let component: Edit;
  let fixture: ComponentFixture<Edit>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [Edit],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    })
    .compileComponents();

    fixture = TestBed.createComponent(Edit);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  // The schema's width/height used to be ignored here entirely, which made the layout
  // shown by the schema editor meaningless.
  it('lays fields out using the width and height from the schema', () => {
    // A fresh fixture: the shared one has already been change-detected, and mutating
    // component state afterwards trips the dev-mode ExpressionChangedAfterItHasBeenChecked
    // check.
    const fresh = TestBed.createComponent(Edit);
    fresh.componentInstance.schema = [
      { name: 'a', field_type: 'Number', required: false, width: 6, height: 2 },
      { name: 'b', field_type: 'Boolean', required: false, width: 6, height: 1 },
    ];
    fresh.detectChanges();

    const cells = fresh.nativeElement.querySelectorAll('.field-cell') as NodeListOf<HTMLElement>;
    expect(cells.length).toBe(2);
    expect(cells[0].style.gridColumn).toBe('span 6');
    expect(cells[0].style.minHeight).toBe('calc(var(--field-row-unit, 72px) * 2)');
    expect(cells[1].style.minHeight).toBe('calc(var(--field-row-unit, 72px) * 1)');
  });

  // Bad input used to be caught by normalising the JSON buffers at save time; now the
  // value fields own their input and report problems, so the refusal lives here.
  it('refuses to save while a field reports a problem', () => {
    const fresh = TestBed.createComponent(Edit);
    const component = fresh.componentInstance;
    component.setFieldError(
      { name: 'numbers', field_type: 'Number', required: false, width: 12, height: 1 },
      "Field 'numbers': invalid JSON",
    );

    component.save();

    expect(component.error()).toContain('invalid JSON');
  });
});
