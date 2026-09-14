import { COMMA, ENTER } from '@angular/cdk/keycodes';
import { Component, model } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatChipEditedEvent, MatChipsModule, MatChipInputEvent } from '@angular/material/chips';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { TranslocoPipe } from '@jsverse/transloco';
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
    TranslocoPipe,
  ],
  templateUrl: './enum-field.html',
  styleUrl: './enum-field.scss',
})
export class EnumField {
  /**
   * Enum options travel as a JSON array on the wire. This used to be a `Set`, which
   * serialises to `{}` and whose in-place mutation did not change the signal value, so
   * neither the UI nor the backend ever saw an update.
   */
  field = model<string[]>([]);
  separatorKeysCodes: number[] = [ENTER, COMMA];
  addOnBlur = true;

  removeEnumOption(value: string) {
    // Return a new array so the signal actually notifies its consumers.
    this.field.update((values) => values.filter((v) => v !== value));
  }

  editEnumOption(oldValue: string, event: MatChipEditedEvent) {
    const newValue = event.value.trim();
    if (!newValue) {
      this.removeEnumOption(oldValue);
      return;
    }
    this.field.update((values) => values.map((v) => (v === oldValue ? newValue : v)));
  }

  addEnumOption(event: MatChipInputEvent) {
    const value = (event.value || '').trim();
    if (value) {
      this.field.update((values) => (values.includes(value) ? values : [...values, value]));
    }
    event.chipInput?.clear();
  }
}
