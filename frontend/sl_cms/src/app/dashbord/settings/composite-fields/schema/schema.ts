import { Component, inject, signal, ChangeDetectionStrategy } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';

import { errorMessage as message } from 'app/core/http-error';
import { FieldSchema } from 'app/models/schema/fields';
import { CompositeFieldsService } from 'app/services/schema/composite_fields.service';
import { EditSchema } from '../../schemas/shared/edit-schema/edit-schema';

/**
 * Schema editor for one composite field definition.
 *
 * Reuses the collections' schema editor, so composites get drag reordering, edge resizing
 * and the layout preview too. A composite's fields can themselves be composite fields, so
 * these definitions nest (the server rejects reference cycles).
 */
@Component({
  selector: 'app-composite-field-schema',
  imports: [RouterLink, MatButtonModule, EditSchema],
  templateUrl: './schema.html',
  changeDetection: ChangeDetectionStrategy.Eager,
  styleUrl: './schema.scss',
})
export class Schema {
  private route = inject(ActivatedRoute);
  private compositeFields = inject(CompositeFieldsService);

  public compositeId: string = this.route.snapshot.params['id'];
  public schema = signal<FieldSchema[]>([]);
  public status = signal('');
  public error = signal('');

  constructor() {
    this.compositeFields.getCompositeFieldSchema(this.compositeId).subscribe({
      next: (schema) => this.schema.set(schema),
      error: (e) => this.error.set(`Failed to load the definition: ${message(e)}`),
    });
  }

  save(schema: FieldSchema[]) {
    this.compositeFields.updateCompositeFieldSchema(this.compositeId, schema).subscribe({
      next: () => {
        // Content forms read the definitions from a cache, so it has to be dropped or they
        // would keep rendering the previous shape.
        this.compositeFields.invalidate();
        this.error.set('');
        this.status.set('Saved');
      },
      error: (e) => {
        this.status.set('');
        this.error.set(`Save failed: ${message(e)}`);
      },
    });
  }
}
