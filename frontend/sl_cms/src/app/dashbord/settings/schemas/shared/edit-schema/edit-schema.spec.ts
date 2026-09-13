import { CdkDragDrop } from '@angular/cdk/drag-drop';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { DefaultFieldLayout, FieldSchema } from 'app/models/schema/fields';
import { EditSchema } from './edit-schema';

function field(name: string): FieldSchema {
  return {
    name,
    field_type: 'Number',
    required: false,
    ...DefaultFieldLayout,
  };
}

describe('EditSchema', () => {
  let component: EditSchema;
  let fixture: ComponentFixture<EditSchema>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [EditSchema],
      // The layout preview renders `ValueField`, which uploads images.
      providers: [provideHttpClient(), provideHttpClientTesting()],
    })
    .compileComponents();

    fixture = TestBed.createComponent(EditSchema);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  it('adds a field with the default layout', () => {
    component.Schema = [];
    component.addField();

    expect(component.Schema).toHaveLength(1);
    expect(component.Schema[0].width).toBe(DefaultFieldLayout.width);
    expect(component.Schema[0].height).toBe(DefaultFieldLayout.height);
  });

  // Array order is display order, so these all have to reorder the schema itself.
  it('reorders on drop', () => {
    component.Schema = [field('a'), field('b'), field('c')];

    component.drop({ previousIndex: 0, currentIndex: 2 } as CdkDragDrop<FieldSchema[]>);

    expect(component.Schema.map((f) => f.name)).toEqual(['b', 'c', 'a']);
  });

  it('ignores a drop that does not move anything', () => {
    component.Schema = [field('a'), field('b')];

    component.drop({ previousIndex: 1, currentIndex: 1 } as CdkDragDrop<FieldSchema[]>);

    expect(component.Schema.map((f) => f.name)).toEqual(['a', 'b']);
  });

  it('moves fields with the arrow buttons, and stops at the ends', () => {
    component.Schema = [field('a'), field('b'), field('c')];

    component.moveDown(0);
    expect(component.Schema.map((f) => f.name)).toEqual(['b', 'a', 'c']);

    component.moveUp(2);
    expect(component.Schema.map((f) => f.name)).toEqual(['b', 'c', 'a']);

    // Out-of-range moves are no-ops rather than errors.
    component.moveUp(0);
    component.moveDown(2);
    expect(component.Schema.map((f) => f.name)).toEqual(['b', 'c', 'a']);
  });

  it('emits the schema when it changes so the parent can persist it', () => {
    component.Schema = [field('a'), field('b')];
    const emitted: FieldSchema[][] = [];
    component.SchemaChange.subscribe((schema) => emitted.push(schema));

    component.moveDown(0);
    component.removeField(0);

    expect(emitted).toHaveLength(2);
    expect(component.Schema.map((f) => f.name)).toEqual(['a']);
  });

  it('places each field on the shared grid using its own width and height', () => {
    // A fresh fixture: the shared one has already been change-detected, and mutating an
    // input afterwards trips the dev-mode ExpressionChangedAfterItHasBeenChecked check.
    const fresh = TestBed.createComponent(EditSchema);
    fresh.componentInstance.Schema = [{ ...field('a'), width: 4, height: 3 }];
    fresh.detectChanges();

    const cell = fresh.nativeElement.querySelector('.schema-field') as HTMLElement;
    expect(cell).toBeTruthy();
    expect(cell.style.gridColumn).toBe('span 4');
    expect(cell.style.minHeight).toBe('calc(var(--field-row-unit, 72px) * 3)');
  });

  /** jsdom has no layout, so the grid is given a width and pointer capture is stubbed. */
  function prepareResize(fresh: ComponentFixture<EditSchema>, width: number, height: number) {
    fresh.componentInstance.Schema = [{ ...field('a'), width, height }];
    fresh.detectChanges();

    const grid = fresh.nativeElement.querySelector('.field-grid') as HTMLElement;
    grid.getBoundingClientRect = () => ({ width: 1200 }) as unknown as DOMRect;

    const handle = (selector: string) => {
      const element = fresh.nativeElement.querySelector(selector) as HTMLElement;
      element.setPointerCapture = () => undefined;
      return element;
    };
    const pointer = (type: string, clientX: number, clientY = 0) =>
      new MouseEvent(type, { clientX, clientY, bubbles: true });

    return { handle, pointer };
  }

  it('resizes a field by dragging its right edge', () => {
    const fresh = TestBed.createComponent(EditSchema);
    const { handle, pointer } = prepareResize(fresh, 4, 1);

    const right = handle('.resize-handle-right');
    right.dispatchEvent(pointer('pointerdown', 0));
    // 1200px / 12 columns = 100px each, so 200px is two columns.
    right.dispatchEvent(pointer('pointermove', 200));
    right.dispatchEvent(pointer('pointerup', 200));

    expect(fresh.componentInstance.Schema[0].width).toBe(6);
    expect(fresh.componentInstance.Schema[0].height).toBe(1);
  });

  it('resizes a field by dragging its bottom edge', () => {
    const fresh = TestBed.createComponent(EditSchema);
    const { handle, pointer } = prepareResize(fresh, 6, 1);

    const bottom = handle('.resize-handle-bottom');
    bottom.dispatchEvent(pointer('pointerdown', 0, 0));
    // One row unit is 72px.
    bottom.dispatchEvent(pointer('pointermove', 0, 144));
    bottom.dispatchEvent(pointer('pointerup', 0, 144));

    expect(fresh.componentInstance.Schema[0].height).toBe(3);
    expect(fresh.componentInstance.Schema[0].width).toBe(6);
  });

  it('emits the schema once a resize ends', () => {
    const fresh = TestBed.createComponent(EditSchema);
    const { handle, pointer } = prepareResize(fresh, 4, 1);
    const emitted: FieldSchema[][] = [];
    fresh.componentInstance.SchemaChange.subscribe((schema) => emitted.push(schema));

    const right = handle('.resize-handle-right');
    right.dispatchEvent(pointer('pointerdown', 0));
    right.dispatchEvent(pointer('pointermove', 100));
    // Moves alone do not emit; the change is reported when the drag ends.
    expect(emitted).toHaveLength(0);
    right.dispatchEvent(pointer('pointerup', 100));

    expect(emitted).toHaveLength(1);
  });

  it('previews the layout with the same widgets the content editor uses', () => {
    const fresh = TestBed.createComponent(EditSchema);
    fresh.componentInstance.Schema = [field('a')];
    fresh.detectChanges();

    // Editing mode shows the field editor.
    expect(fresh.nativeElement.querySelector('app-field')).toBeTruthy();
    expect(fresh.nativeElement.querySelector('app-value-field')).toBeFalsy();

    fresh.componentInstance.togglePreview();
    fresh.detectChanges();

    expect(fresh.nativeElement.querySelector('app-value-field')).toBeTruthy();
    expect(fresh.nativeElement.querySelector('app-field')).toBeFalsy();
  });
});
