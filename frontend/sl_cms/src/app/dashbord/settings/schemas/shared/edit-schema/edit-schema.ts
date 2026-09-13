import { Component, EventEmitter, Input, Output } from '@angular/core';
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

import { fieldCellStyle } from 'app/core/field-layout';
import { DefaultFieldLayout, FieldDefaults, FieldSchema } from 'app/models/schema/fields';
import { Field } from '../field/field';

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
  ],
  templateUrl: './edit-schema.html',
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
}
