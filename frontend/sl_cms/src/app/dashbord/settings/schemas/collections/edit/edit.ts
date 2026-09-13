import { Component, inject, signal } from '@angular/core';
import { EditSchema } from "../../shared/edit-schema/edit-schema";
import { CollectionsService } from 'app/services/schema/collections.service';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { ActivatedRoute } from '@angular/router';

@Component({
  selector: 'app-edit',
  imports: [
    EditSchema,
  ],
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
  public status = signal('');
  public error = signal('');

  constructor() {
    this.collectionsService.getCollectionSchema(this.collectionName).subscribe({
      next: (schema: CollectionSchema) => this.collectionSchema.set(schema),
      error: (e) => this.error.set(`Load failed: ${e?.error ?? e?.message ?? e}`),
    });
  }

  save(schema: FieldSchema[]) {
    this.collectionsService.updateCollectionSchema(this.collectionName, schema).subscribe({
      next: () => {
        this.error.set('');
        this.status.set('Saved');
      },
      error: (e) => {
        this.status.set('');
        this.error.set(`Save failed: ${e?.error ?? e?.message ?? e}`);
      },
    });
  }
}
