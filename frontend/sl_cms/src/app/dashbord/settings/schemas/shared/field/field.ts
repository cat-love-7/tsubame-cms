import { Component, EventEmitter, Input, Output, ChangeDetectionStrategy } from '@angular/core';
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
  FieldType,
  FieldTypeStringPipe,
  IsArrayFieldSchemaPipe,
  IsCompositeFieldSchemaPipe,
  IsEnumFieldSchemaPipe,
  IsMarkdownFieldSchema,
  IsTextFieldSchema,
  isArrayFieldSchema,
  reconcileArrayItemTypes,
} from 'app/models/schema/fields';
import { EnumField } from "../enum-field/enum-field";
import { CompositeField } from '../composite-field/composite-field';
import { TextField } from "../text-field/text-field";
import { FieldWidthPresets } from 'app/core/field-layout';

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
  changeDetection: ChangeDetectionStrategy.Eager,
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
  public widthPresets = FieldWidthPresets;

  /** Typing a raw column count is not intuitive; the presets cover the common fractions. */
  public setWidth(width: number) {
    this.field.width = width;
    this.fieldChange.emit(this.field);
  }

  public onFieldTypeChange(value: keyof typeof FieldDefaults) {
    this.field.field_type = FieldDefaults[value];
    this.fieldChange.emit(this.field);
  }

  /**
   * Array item types are untyped on the wire, so `Number` and `Image` cannot be combined
   * (an image id is a number). The binding is one-way, so `field.field_type.Array` is
   * still the previous selection here.
   */
  public onArrayItemTypesChange(selected: FieldType[]) {
    const fieldType = this.field.field_type;
    if (!isArrayFieldSchema(fieldType)) {
      return;
    }
    fieldType.Array = reconcileArrayItemTypes(fieldType.Array, selected);
    this.fieldChange.emit(this.field);
  }
}
