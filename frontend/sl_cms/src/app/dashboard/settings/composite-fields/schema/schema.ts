import { Component, DestroyRef, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { TranslocoPipe } from '@jsverse/transloco';

import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { HasUnsavedChanges } from 'app/core/unsaved-changes.guard';
import { fingerprint } from 'app/core/value-changes';
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
export class CompositeFieldSchema implements HasUnsavedChanges {
  private route = inject(ActivatedRoute);
  private compositeFields = inject(CompositeFieldsService);
  /** When this screen goes away, so does everything it still has in flight. */
  private destroyRef = inject(DestroyRef);

  /** A signal, and read from the parameter stream: switching definitions reuses this component. */
  public compositeId = signal('');
  public schema = signal<FieldSchema[]>([]);
  public status = signal<Message | null>(null);
  public error = signal<Message | null>(null);
  /** Which visit to a definition the answers on screen belong to (see `load`). */
  private loadToken = 0;
  /** What the schema held when it was last in step with the server (see the collection editor). */
  private saved = signal('');

  constructor() {
    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const id = params.get('id') ?? '';
      if (id !== this.compositeId()) {
        this.load(id);
      }
    });
  }

  /** Everything on screen belongs to one definition, so a switch starts from nothing. */
  private load(id: string) {
    // See the collection schema editor: a slow answer for the definition the reader left must not
    // be edited here and written over this definition.
    const token = ++this.loadToken;
    this.compositeId.set(id);
    this.schema.set([]);
    this.status.set(null);
    this.error.set(null);

    this.compositeFields
      .getCompositeFieldSchema(id)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (schema) => {
          if (token === this.loadToken) {
            this.schema.set(schema);
            this.saved.set(fingerprint(schema));
          }
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.failedToLoadDefinition', e));
          }
        },
      });
  }

  save(schema: FieldSchema[]) {
    const started = { id: this.compositeId(), generation: this.loadToken };
    this.compositeFields
      .updateCompositeFieldSchema(started.id, schema)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: () => {
          if (!this.stillOn(started)) {
            return;
          }
          // Content forms read the definitions from a cache, so it has to be dropped or they
          // would keep rendering the previous shape.
          this.compositeFields.invalidate();
          this.error.set(null);
          this.status.set(t('common.saved'));
          this.saved.set(fingerprint(this.schema()));
        },
        error: (e) => {
          if (!this.stillOn(started)) {
            return;
          }
          this.status.set(null);
          this.error.set(failure('content.saveFailed', e));
        },
      });
  }

  /** Whether the schema holds edits that would be lost by leaving (see `unsavedChangesGuard`). */
  hasUnsavedChanges(): boolean {
    return fingerprint(this.schema()) !== this.saved();
  }

  /** Whether the screen is still on the definition a slow answer was about, as it was then. */
  private stillOn(started: { id: string; generation: number }): boolean {
    return this.compositeId() === started.id && this.loadToken === started.generation;
  }
}
