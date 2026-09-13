import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { BehaviorSubject, forkJoin, switchMap } from 'rxjs';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';

import { errorMessage as message } from 'app/core/http-error';
import { ItemMetadataMap, ItemStatus } from 'app/models/item-status';
import { CollectionSchema } from 'app/models/schema/collection';
import { CollectionItemEntry } from 'app/models/values/collection';
import { formatFieldValue } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';

@Component({
  selector: 'app-collection-items',
  imports: [MatButtonModule, MatIconModule, RouterLink, ItemStatusBadge],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class List {
  private route = inject(ActivatedRoute);
  private collectionsService = inject(CollectionsService);

  public collectionName: string = this.route.snapshot.params['name'];
  public schema: CollectionSchema = [];
  public items: CollectionItemEntry[] = [];
  public error = signal('');
  /** Draft/published state per item id; the server sends drafts for untouched items. */
  public metadata: ItemMetadataMap = {};
  /** Exposed for the template. */
  public format = formatFieldValue;

  /** Re-issues the list request after a delete. */
  private refresh = new BehaviorSubject<void>(undefined);

  constructor() {
    this.collectionsService.getCollectionSchema(this.collectionName).subscribe({
      next: (schema) => (this.schema = schema),
      error: (e) => this.error.set(`Failed to load the schema: ${message(e)}`),
    });

    // Items and their status are fetched together: the table shows both in the same row.
    this.refresh
      .pipe(
        switchMap(() =>
          forkJoin({
            items: this.collectionsService.listCollectionItems(this.collectionName),
            metadata: this.collectionsService.listItemMetadata(this.collectionName),
          }),
        ),
      )
      .subscribe({
        next: ({ items, metadata }) => {
          this.items = items;
          this.metadata = metadata;
        },
        error: (e) => this.error.set(`Failed to load the items: ${message(e)}`),
      });
  }

  statusOf(id: number): ItemStatus {
    return this.metadata[String(id)]?.status ?? 'draft';
  }

  /** When the item's values were last saved, or a dash when that was never recorded. */
  updatedAt(id: number): string {
    const updated = this.metadata[String(id)]?.updated_at;
    return updated ? new Date(updated).toLocaleString() : '—';
  }

  /** Publish or unpublish one item, without leaving the list. */
  togglePublished(id: number) {
    const request =
      this.statusOf(id) === 'published'
        ? this.collectionsService.unpublishItem(this.collectionName, id)
        : this.collectionsService.publishItem(this.collectionName, id);

    request.subscribe({
      next: (metadata) => {
        this.error.set('');
        this.metadata = { ...this.metadata, [String(id)]: metadata };
      },
      error: (e) => this.error.set(`Could not change the published state: ${message(e)}`),
    });
  }

  delete(id: number) {
    if (!confirm(`Delete item ${id}?`)) {
      return;
    }
    this.collectionsService.deleteCollectionItem(this.collectionName, id).subscribe({
      next: () => {
        this.error.set('');
        this.refresh.next();
      },
      error: (e) => this.error.set(`Delete failed: ${message(e)}`),
    });
  }
}
