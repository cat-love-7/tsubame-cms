import { Component, EventEmitter, Input, Output } from '@angular/core';
import { Field } from '../field/field';
import { FieldDefaults, FieldSchema } from 'app/models/schema/fields';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatButtonModule } from '@angular/material/button';
import { MatGridListModule } from '@angular/material/grid-list';
import { CdkDrag, CdkDropList } from '@angular/cdk/drag-drop';

@Component({
  selector: 'app-edit-schema',
  imports: [
    Field,
    MatIconModule,
    MatInputModule,
    MatButtonModule,
    MatGridListModule,
    CdkDropList,
    CdkDrag,
  ],
  templateUrl: './edit-schema.html',
  styleUrl: './edit-schema.scss',
})
export class EditSchema {
  @Input() Schema: FieldSchema[] = [];
  @Output() SchemaChange = new EventEmitter<FieldSchema[]>();

  addField() {
    this.Schema.push({ name: '', field_type: FieldDefaults.Text, required: false });
    this.SchemaChange.emit(this.Schema);
  }
  saveSchema() {
    console.log('Schema saved:', this.Schema);
  }
}
