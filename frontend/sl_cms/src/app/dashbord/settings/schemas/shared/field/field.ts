import { Component, EventEmitter, Input, Output } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatChipsModule } from '@angular/material/chips';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import {
  ArrayItemTypeOptions,
  DefaultFieldLayout,
  FieldDefaults,
  FieldSchema,
  FieldTypeStringPipe,
  IsArrayFieldSchemaPipe,
  IsCompositeFieldSchemaPipe,
  IsEnumFieldSchemaPipe,
  IsMarkdownFieldSchema,
  IsTextFieldSchema,
} from 'app/models/schema/fields';
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
    IsArrayFieldSchemaPipe,
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
  @Input() field: FieldSchema = {
    name: '',
    field_type: FieldDefaults.Text,
    required: false,
    ...DefaultFieldLayout,
  };
  @Output() fieldChange = new EventEmitter<FieldSchema>();
  public typeOptions = Object.keys(FieldDefaults);
  public arrayItemTypeOptions = ArrayItemTypeOptions;

  public onFieldTypeChange(value: keyof typeof FieldDefaults) {
    this.field.field_type = FieldDefaults[value];
    this.fieldChange.emit(this.field);
  }
}
