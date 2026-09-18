import { Component, DestroyRef, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { TranslocoPipe } from '@jsverse/transloco';

import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { HasUnsavedChanges } from 'app/core/unsaved-changes.guard';
import { fingerprint } from 'app/core/value-changes';
import { FieldSchema } from 'app/models/schema/fields';
import { SinglePagesService } from 'app/services/schema/single-pages.service';
import { EditSchema } from '../../shared/edit-schema/edit-schema';

/** Schema editor for one single page. Reuses the schema editor the collections use. */
@Component({
  selector: 'app-single-page-schema',
  imports: [EditSchema, MatButtonModule, MessagePipe, RouterLink, TranslocoPipe],
  templateUrl: './schema.html',
  styleUrl: './schema.scss',
})
export class SinglePageSchema implements HasUnsavedChanges {
  private route = inject(ActivatedRoute);
  private pages = inject(SinglePagesService);
  /** When this screen goes away, so does everything it still has in flight. */
  private destroyRef = inject(DestroyRef);

  /** A signal, and read from the parameter stream: switching pages reuses this component. */
  public pageName = signal('');
  public schema = signal<FieldSchema[]>([]);
  public status = signal<Message | null>(null);
  public error = signal<Message | null>(null);
  /** Which visit to a schema the answers on screen belong to (see `load`). */
  private loadToken = 0;
  /** What the schema held when it was last in step with the server (see the collection editor). */
  private saved = signal('');

  constructor() {
    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const name = params.get('name') ?? '';
      if (name !== this.pageName()) {
        this.load(name);
      }
    });
  }

  /** Everything on screen belongs to one page's schema, so a switch starts from nothing. */
  private load(name: string) {
    // See the collection schema editor: a slow answer for the page the reader left must not be
    // edited here and written over this page's schema.
    const token = ++this.loadToken;
    this.pageName.set(name);
    this.schema.set([]);
    this.status.set(null);
    this.error.set(null);

    this.pages
      .getPageSchema(name)
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
            this.error.set(failure('content.failedToLoadSchema', e));
          }
        },
      });
  }

  save(schema: FieldSchema[]) {
    const started = this.start();
    this.pages
      .updatePageSchema(started.name, schema)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: () => {
          if (!this.stillOn(started)) {
            return;
          }
          this.error.set(null);
          this.status.set(t('common.saved'));
          this.saved.set(fingerprint(this.schema()));
        },
        error: (e) => {
          if (this.stillOn(started)) {
            this.status.set(null);
            this.error.set(failure('content.saveFailed', e));
          }
        },
      });
  }

  /** The schema an act is about, and the load it belongs to, captured when the act starts. */
  private start(): { name: string; generation: number } {
    return { name: this.pageName(), generation: this.loadToken };
  }

  /** Whether the schema holds edits that would be lost by leaving (see `unsavedChangesGuard`). */
  hasUnsavedChanges(): boolean {
    return fingerprint(this.schema()) !== this.saved();
  }

  /** Whether the screen is still on the schema a slow answer was about, as it was then. */
  private stillOn(started: { name: string; generation: number }): boolean {
    return this.pageName() === started.name && this.loadToken === started.generation;
  }
}
