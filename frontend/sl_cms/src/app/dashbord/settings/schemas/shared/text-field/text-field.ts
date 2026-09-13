import { Component, EventEmitter, Input, Output } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { TextFieldOptions } from 'app/models/schema/fields';

@Component({
  selector: 'app-text-field',
  imports: [
    MatFormFieldModule,
    MatInputModule,
    FormsModule,
  ],
  templateUrl: './text-field.html',
  styleUrl: './text-field.scss',
})
export class TextField {
  @Input() fieldOptions: TextFieldOptions = {};
  @Output() fieldOptionsChange = new EventEmitter<TextFieldOptions>();
}
