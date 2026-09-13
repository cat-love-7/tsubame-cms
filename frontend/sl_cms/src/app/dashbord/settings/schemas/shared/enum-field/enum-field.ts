import { COMMA, ENTER } from '@angular/cdk/keycodes';
import { Component, model } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatChipEditedEvent, MatChipsModule, MatChipInputEvent } from '@angular/material/chips';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';

@Component({
  selector: 'app-enum-field',
  imports: [
    FormsModule,
    MatFormFieldModule,
    MatInputModule,
    MatSelectModule,
    MatCheckboxModule,
    MatChipsModule,
    MatIconModule,
  ],
  templateUrl: './enum-field.html',
  styleUrl: './enum-field.scss',
})
export class EnumField {
  field = model<Set<string>>(new Set());
  separatorKeysCodes: number[] = [ENTER, COMMA];
  addOnBlur = true;
  removeEnumOption(value: string) {
    this.field.update((field) => {
      field.delete(value);
      return field;
    });
  }
  editEnumOption(oldValue: string, event: MatChipEditedEvent) {
    const newValue = event.value.trim();
    this.field.update((field) => {
      if (field.has(oldValue) && newValue) {
        field.delete(oldValue);
        field.add(newValue);
      }
      return field;
    });
  }
  addEnumOption(event: MatChipInputEvent) {
    const value = (event.value || '').trim();
    this.field.update((field) => {
      if (value) {
        field.add(value);
      }
      event.chipInput!.clear();
      return field;
    });
  }
}
