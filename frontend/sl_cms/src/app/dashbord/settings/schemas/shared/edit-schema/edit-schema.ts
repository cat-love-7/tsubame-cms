import { Component, EventEmitter, Input, Output } from '@angular/core';
import { Field } from '../field/field';
import { DefaultFieldLayout, FieldDefaults, FieldSchema } from 'app/models/schema/fields';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatButtonModule } from '@angular/material/button';
import { MatGridListModule } from '@angular/material/grid-list';
import { CdkDropList } from '@angular/cdk/drag-drop';

@Component({
  selector: 'app-edit-schema',
  imports: [
    Field,
    MatIconModule,
    MatInputModule,
    MatButtonModule,
    MatGridListModule,
    CdkDropList,
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

  requestSave() {
    this.save.emit(this.Schema);
  }
}
