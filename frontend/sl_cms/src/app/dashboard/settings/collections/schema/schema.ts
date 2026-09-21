import { Component, DestroyRef, inject, signal } from '@angular/core';
import { TranslocoPipe } from '@jsverse/transloco';
import { EditSchema } from '../../shared/edit-schema/edit-schema';
import { CollectionsService } from 'app/services/schema/collections.service';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { forkJoin } from 'rxjs';
import { ActivatedRoute } from '@angular/router';

import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { HasUnsavedChanges } from 'app/core/unsaved-changes.guard';
import { fingerprint } from 'app/core/value-changes';

@Component({
  selector: 'app-collection-schema-edit',
  imports: [EditSchema, MessagePipe, TranslocoPipe],
  templateUrl: './schema.html',
  styleUrl: './schema.scss',
})
export class CollectionSchemaEdit implements HasUnsavedChanges {
  private route = inject(ActivatedRoute);
  private collectionsService = inject(CollectionsService);
  /** When this screen goes away, so does everything it still has in flight. */
  private destroyRef = inject(DestroyRef);
  /** A signal, and read from the parameter stream: switching collections reuses this component. */
  public collectionName = signal('');
  /**
   * Kept as a plain field rather than an `async` pipe binding: `(obs | async) || []`
   * would hand the child a fresh array on every change detection pass until the request
   * resolves, discarding fields the user had already added.
   */
  public collectionSchema = signal<FieldSchema[]>([]);
  /**
   * Whether a preview link may be minted for this collection's working copies.
   *
   * A plain signal rather than part of the schema array: it is saved beside the fields, and the
   * server keeps the two apart for the same reason.
   */
  public preview = signal(false);
  /** What the last save did, or the failure to show: keys, so they follow a language change. */
  public status = signal<Message | null>(null);
  public error = signal<Message | null>(null);
  /** Which visit to a schema the answers on screen belong to (see `load`). */
  private loadToken = 0;
  /**
   * What the schema held when it was last in step with the server.
   *
   * The child editor mutates the array it is given (fields added, moved, resized), so a signal
   * would not see it: the comparison is a rendering of the two, and `hasUnsavedChanges` is asked
   * for on the way out rather than watched.
   */
  private saved = signal('');

  constructor() {
    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const name = params.get('name') ?? '';
      if (name !== this.collectionName()) {
        this.load(name);
      }
    });
  }

  /** Everything on screen belongs to one schema, so a switch starts from nothing. */
  private load(name: string) {
    // Switching schemas reuses this component, and the schema on screen is what `save` sends: a
    // slow answer for the schema the reader left would otherwise be edited here and written over
    // *this* collection's. The generation is what says which answer is still wanted.
    const token = ++this.loadToken;
    this.collectionName.set(name);
    this.collectionSchema.set([]);
    this.preview.set(false);
    this.status.set(null);
    this.error.set(null);

    this.collectionsService
      .getCollectionSchema(name)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (schema: CollectionSchema) => {
          if (token === this.loadToken) {
            this.collectionSchema.set(schema);
            this.saved.set(this.fingerprintNow());
          }
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.loadFailed', e));
          }
        },
      });

    // Asked for separately rather than alongside the schema: a deployment that predates settings
    // would fail the pair, and then a schema that arrived perfectly well would not be shown.
    this.collectionsService
      .getCollectionSettings(name)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (settings) => {
          if (token === this.loadToken) {
            this.preview.set(settings.preview);
            this.saved.set(this.fingerprintNow());
          }
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.loadFailed', e));
          }
        },
      });
  }

  /** Everything the screen would save, as one string: what "unchanged" is measured by. */
  private fingerprintNow(): string {
    return fingerprint({ schema: this.collectionSchema(), preview: this.preview() });
  }

  save(schema: FieldSchema[]) {
    const started = this.start();
    // One press saves both: the fields an editor changed and the setting they turned on are one
    // act from where they are sitting, even though the server keeps them in two records.
    forkJoin([
      this.collectionsService.updateCollectionSchema(started.name, schema),
      this.collectionsService.updateCollectionSettings(started.name, { preview: this.preview() }),
    ])
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: () => {
          if (!this.stillOn(started)) {
            return;
          }
          this.error.set(null);
          this.status.set(t('common.saved'));
          this.saved.set(this.fingerprintNow());
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
    return { name: this.collectionName(), generation: this.loadToken };
  }

  /** Whether the schema holds edits that would be lost by leaving (see `unsavedChangesGuard`). */
  hasUnsavedChanges(): boolean {
    return this.fingerprintNow() !== this.saved();
  }

  /** Whether the screen is still on the schema a slow answer was about, as it was then. */
  private stillOn(started: { name: string; generation: number }): boolean {
    return this.collectionName() === started.name && this.loadToken === started.generation;
  }
}
