import { Component, EventEmitter, Input, Output } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { TranslocoPipe } from '@jsverse/transloco';
import { TextFieldOptions } from 'app/models/schema/fields';

@Component({
  selector: 'app-text-field',
  imports: [MatCheckboxModule, MatFormFieldModule, MatInputModule, FormsModule, TranslocoPipe],
  templateUrl: './text-field.html',
  styleUrl: './text-field.scss',
})
export class TextField {
  @Input() fieldOptions: TextFieldOptions = {};
  /**
   * Whether this is a plain `Text` field, which may be written over several lines.
   *
   * Markdown is multi-line by nature, so the choice is not offered for it: a Markdown field that
   * could be one line is a `Text` field.
   */
  @Input() multilineAllowed = false;
  @Output() fieldOptionsChange = new EventEmitter<TextFieldOptions>();
}
