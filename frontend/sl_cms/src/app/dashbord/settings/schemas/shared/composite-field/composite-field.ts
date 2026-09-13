import { Component, inject, model } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatChipsModule } from '@angular/material/chips';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { CompositeFieldsService } from 'app/services/schema/composite_fields.service';
import { AsyncPipe, KeyValuePipe } from '@angular/common';

@Component({
  selector: 'app-composite-field',
  imports: [
    FormsModule,
    MatFormFieldModule,
    MatInputModule,
    MatSelectModule,
    MatCheckboxModule,
    MatChipsModule,
    MatIconModule,
    AsyncPipe,
    KeyValuePipe,
  ],
  templateUrl: './composite-field.html',
  styleUrl: './composite-field.scss',
})
export class CompositeField {
  field = model<string>('');
    
  private compositeFieldsService = inject(CompositeFieldsService);
  public compositeFields = this.compositeFieldsService.getAllCompositeFields();
  
}
