import { CdkDragDrop } from '@angular/cdk/drag-drop';
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
      imports: [EditSchema]
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
});
