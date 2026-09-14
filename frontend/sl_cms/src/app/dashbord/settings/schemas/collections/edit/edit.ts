import { Component, inject, signal } from '@angular/core';
import { EditSchema } from "../../shared/edit-schema/edit-schema";
import { CollectionsService } from 'app/services/schema/collections.service';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { ActivatedRoute } from '@angular/router';

import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';

@Component({
  selector: 'app-edit',
  imports: [EditSchema, MessagePipe],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class Edit {
  private route = inject(ActivatedRoute);
  private collectionsService = inject(CollectionsService);
  public collectionName: string = this.route.snapshot.params['name'];
  /**
   * Kept as a plain field rather than an `async` pipe binding: `(obs | async) || []`
   * would hand the child a fresh array on every change detection pass until the request
   * resolves, discarding fields the user had already added.
   */
  public collectionSchema = signal<FieldSchema[]>([]);
  /** What the last save did, or the failure to show: keys, so they follow a language change. */
  public status = signal<Message | null>(null);
  public error = signal<Message | null>(null);

  constructor() {
    this.collectionsService.getCollectionSchema(this.collectionName).subscribe({
      next: (schema: CollectionSchema) => this.collectionSchema.set(schema),
      error: (e) => this.error.set(failure('content.loadFailed', e)),
    });
  }

  save(schema: FieldSchema[]) {
    this.collectionsService.updateCollectionSchema(this.collectionName, schema).subscribe({
      next: () => {
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
