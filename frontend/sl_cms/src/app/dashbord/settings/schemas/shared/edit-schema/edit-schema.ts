import { Component, EventEmitter, Input, Output, signal, ChangeDetectionStrategy } from '@angular/core';
import {
  CdkDrag,
  CdkDragDrop,
  CdkDragHandle,
  CdkDropList,
  moveItemInArray,
} from '@angular/cdk/drag-drop';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatButtonModule } from '@angular/material/button';

import {
  FIELD_ROW_UNIT,
  MAX_FIELD_WIDTH,
  fieldCellStyle,
  resizeHeight,
  resizeWidth,
} from 'app/core/field-layout';
import { DefaultFieldLayout, FieldDefaults, FieldSchema } from 'app/models/schema/fields';
import { FieldValue, defaultValueForField } from 'app/models/values/fields';
import { ValueField } from 'app/shared/value-field/value-field';
import { Field } from '../field/field';

/** Which edge of a tile is being dragged. */
type ResizeAxis = 'width' | 'height';

interface ResizeState {
  field: FieldSchema;
  axis: ResizeAxis;
  startX: number;
  startY: number;
  startWidth: number;
  startHeight: number;
  /** Width of one grid column, in pixels. */
  columnWidth: number;
  /** Height of one row unit, in pixels. */
  rowUnit: number;
}

@Component({
  selector: 'app-edit-schema',
  imports: [
    Field,
    MatIconModule,
    MatInputModule,
    MatButtonModule,
    // `mat-grid-list` used to be here, but it renders every tile as 12x1 regardless of
    // the field's width/height, so it showed a layout that no screen ever rendered.
    CdkDropList,
    CdkDrag,
    CdkDragHandle,
    ValueField,
  ],
  templateUrl: './edit-schema.html',
  changeDetection: ChangeDetectionStrategy.Eager,
  styleUrl: './edit-schema.scss',
})
export class EditSchema {
  @Input() Schema: FieldSchema[] = [];
  @Output() SchemaChange = new EventEmitter<FieldSchema[]>();
  /**
   * Emitted when the user asks to persist. The parent performs the HTTP call because it
   * is the component that knows the collection name; this component used to only
   * `console.log` the schema.
   */
  @Output() save = new EventEmitter<FieldSchema[]>();

  /** Exposed for the template: places each field on the shared 12-column grid. */
  public cellStyle = fieldCellStyle;

  private resizeState: ResizeState | null = null;

  /** When on, tiles show the content input instead of the field editor. */
  public preview = signal(false);

  /**
   * Preview values, computed once when preview mode is entered.
   *
   * They have to be stable: recomputing on every change detection would hand the value
   * fields a fresh array each pass and reset the array editor's buffer continuously.
   */
  private previewValues = new Map<string, FieldValue>();

  togglePreview() {
    if (!this.preview()) {
      this.previewValues = new Map(
        this.Schema.map((field) => [field.name, defaultValueForField(field)]),
      );
    }
    this.preview.set(!this.preview());
  }

  previewValue(field: FieldSchema): FieldValue {
    return this.previewValues.get(field.name) ?? null;
  }

  addField() {
    this.Schema.push({
      name: '',
      field_type: FieldDefaults.Text,
      required: false,
      ...DefaultFieldLayout,
    });
    this.SchemaChange.emit(this.Schema);
  }

  removeField(index: number) {
    this.Schema.splice(index, 1);
    this.SchemaChange.emit(this.Schema);
  }

  /** Array order *is* display order, so reordering is a schema change. */
  drop(event: CdkDragDrop<FieldSchema[]>) {
    if (event.previousIndex === event.currentIndex) {
      return;
    }
    moveItemInArray(this.Schema, event.previousIndex, event.currentIndex);
    this.SchemaChange.emit(this.Schema);
  }

  /** Keyboard-accessible alternative to dragging (CDK drag-drop is pointer-only). */
  moveUp(index: number) {
    this.move(index, index - 1);
  }

  moveDown(index: number) {
    this.move(index, index + 1);
  }

  private move(from: number, to: number) {
    if (to < 0 || to >= this.Schema.length) {
      return;
    }
    moveItemInArray(this.Schema, from, to);
    this.SchemaChange.emit(this.Schema);
  }

  requestSave() {
    this.save.emit(this.Schema);
  }

  /**
   * Start a resize from a tile edge.
   *
   * Pointer capture plus template event bindings are used (rather than window listeners)
   * so the moves arrive through Angular's event system and still refresh the view in this
   * zoneless application.
   */
  startResize(field: FieldSchema, axis: ResizeAxis, event: PointerEvent) {
    const target = event.target as HTMLElement;
    const grid = target.closest('.field-grid') as HTMLElement | null;
    if (!grid) {
      return;
    }

    // Keep the browser from selecting text while dragging.
    event.preventDefault();
    target.setPointerCapture?.(event.pointerId);

    const rowUnit =
      Number.parseFloat(getComputedStyle(grid).getPropertyValue('--field-row-unit')) ||
      Number.parseFloat(FIELD_ROW_UNIT) ||
      72;

    this.resizeState = {
      field,
      axis,
      startX: event.clientX,
      startY: event.clientY,
      startWidth: field.width,
      startHeight: field.height,
      columnWidth: grid.getBoundingClientRect().width / MAX_FIELD_WIDTH,
      rowUnit,
    };
  }

  onResizeMove(event: PointerEvent) {
    const state = this.resizeState;
    if (!state) {
      return;
    }
    if (state.axis === 'width') {
      state.field.width = resizeWidth(
        state.startWidth,
        event.clientX - state.startX,
        state.columnWidth,
      );
    } else {
      state.field.height = resizeHeight(
        state.startHeight,
        event.clientY - state.startY,
        state.rowUnit,
      );
    }
  }

  endResize() {
    if (!this.resizeState) {
      return;
    }
    this.resizeState = null;
    // A resize is a schema change like any other.
    this.SchemaChange.emit(this.Schema);
  }
}
