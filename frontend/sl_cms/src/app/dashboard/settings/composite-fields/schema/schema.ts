import { Component, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { TranslocoPipe } from '@jsverse/transloco';

import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { FieldSchema } from 'app/models/schema/fields';
import { CompositeFieldsService } from 'app/services/schema/composite-fields.service';
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
  imports: [EditSchema, MatButtonModule, MessagePipe, RouterLink, TranslocoPipe],
  templateUrl: './schema.html',
  styleUrl: './schema.scss',
})
export class CompositeFieldSchema {
  private route = inject(ActivatedRoute);
  private compositeFields = inject(CompositeFieldsService);

  /** A signal, and read from the parameter stream: switching definitions reuses this component. */
  public compositeId = signal('');
  public schema = signal<FieldSchema[]>([]);
  public status = signal<Message | null>(null);
  public error = signal<Message | null>(null);

  constructor() {
    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const id = params.get('id') ?? '';
      if (id !== this.compositeId()) {
        this.compositeId.set(id);
        this.schema.set([]);
        this.status.set(null);
        this.error.set(null);
        this.compositeFields.getCompositeFieldSchema(id).subscribe({
          next: (schema) => this.schema.set(schema),
          error: (e) => this.error.set(failure('content.failedToLoadDefinition', e)),
        });
      }
    });
  }

  save(schema: FieldSchema[]) {
    this.compositeFields.updateCompositeFieldSchema(this.compositeId(), schema).subscribe({
      next: () => {
        // Content forms read the definitions from a cache, so it has to be dropped or they
        // would keep rendering the previous shape.
        this.compositeFields.invalidate();
        this.error.set(null);
        this.status.set(t('common.saved'));
      },
      error: (e) => {
        this.status.set(null);
        this.error.set(failure('content.saveFailed', e));
      },
    });
  }
}
