import { COMMA, ENTER } from '@angular/cdk/keycodes';
import { Component, EventEmitter, inject, Input, Output } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatChipsModule } from '@angular/material/chips';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { FieldDefaults, FieldSchema, FieldTypeStringPipe, IsArrayFieldSchemaSchemaPipe, IsCompositeFieldSchemaPipe, IsEnumFieldSchemaPipe, IsMarkdownFieldSchema, IsTextFieldSchema } from 'app/models/schema/fields';
import { EnumField } from "../enum-field/enum-field";
import { CompositeField } from '../composite-field/composite-field';
import { TextField } from "../text-field/text-field";

@Component({
  selector: 'app-field',
  imports: [
    FormsModule,
    MatFormFieldModule,
    MatInputModule,
    MatSelectModule,
    MatCheckboxModule,
    MatChipsModule,
    MatIconModule,
    IsTextFieldSchema,
    IsMarkdownFieldSchema,
    IsCompositeFieldSchemaPipe,
    IsArrayFieldSchemaSchemaPipe,
    IsEnumFieldSchemaPipe,
    FieldTypeStringPipe,
    EnumField,
    CompositeField,
    TextField
],
  templateUrl: './field.html',
  styleUrl: './field.scss',
})
export class Field {
  @Input() field: FieldSchema = { name: '', field_type: FieldDefaults.Text,required: false };
  @Output() fieldChange = new EventEmitter<FieldSchema>();
  public typeOptions = Object.keys(FieldDefaults);
  readonly addOnBlur = true;
  readonly separatorKeysCodes = [ENTER, COMMA] as const;

  public onFieldTypeChange(value: keyof typeof FieldDefaults) {
    console.log('Field type changed to:', value);
    this.field.field_type = FieldDefaults[value];
  }

  changeField() {
    this.fieldChange.emit(this.field);
  }
}
