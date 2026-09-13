import { Component, computed, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { BehaviorSubject, forkJoin, switchMap } from 'rxjs';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatPaginatorModule, PageEvent } from '@angular/material/paginator';

import { AuthService } from 'app/core/auth/auth.service';
import { errorMessage as message } from 'app/core/http-error';
import { ItemMetadataMap, ItemStatus } from 'app/models/item-status';
import { CollectionSchema } from 'app/models/schema/collection';
import { CollectionItemEntry } from 'app/models/values/collection';
import { formatFieldValue } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';

/** Rows per page. Small enough to read, large enough to scan. */
const DEFAULT_PAGE_SIZE = 25;

@Component({
  selector: 'app-collection-items',
  imports: [MatButtonModule, MatIconModule, MatPaginatorModule, RouterLink, ItemStatusBadge],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class List {
  private route = inject(ActivatedRoute);
  private collectionsService = inject(CollectionsService);
  /** What the signed-in account may do; the server enforces the same rules. */
  public auth = inject(AuthService);

  public collectionName: string = this.route.snapshot.params['name'];
  public schema = signal<CollectionSchema>([]);
  /**
   * The rows on screen. A signal, not a plain field: the response can land while Angular
   * is checking the view, and a plain field changing then trips
   * `ExpressionChangedAfterItHasBeenCheckedError` — which in dev mode left the row's
   * bindings unapplied (empty cells) when the last page went from 25 rows to one.
   */
  public items = signal<CollectionItemEntry[]>([]);
  public error = signal('');
  /** Draft/published state per item id; the server sends drafts for untouched items. */
  public metadata = signal<ItemMetadataMap>({});

  /** What this account may do *with this collection*, overrides included. */
  public canEdit = computed(() => this.auth.canEditIn('collections', this.collectionName));
  public canPublish = computed(() => this.auth.canPublishIn('collections', this.collectionName));
  /** Items in the collection, not just on this page. Drives the paginator. */
  public total = signal(0);
  public pageIndex = signal(0);
  public pageSize = signal(DEFAULT_PAGE_SIZE);
  public readonly pageSizeOptions = [10, 25, 50, 100];
  /** Exposed for the template. */
  public format = formatFieldValue;

  /** Re-issues the page request after a delete or a page change. */
  private reload = new BehaviorSubject<void>(undefined);

  constructor() {
    this.collectionsService.getCollectionSchema(this.collectionName).subscribe({
      next: (schema) => this.schema.set(schema),
      error: (e) => this.error.set(`Failed to load the schema: ${message(e)}`),
    });

    // Rows, their status and the total are fetched together: the table and the pager both
    // need them, and a collection can be too long to send in one response.
    this.reload
      .pipe(
        switchMap(() =>
          forkJoin({
            page: this.collectionsService.listCollectionItemsPage(this.collectionName, {
              limit: this.pageSize(),
              offset: this.pageIndex() * this.pageSize(),
            }),
            metadata: this.collectionsService.listItemMetadata(this.collectionName),
          }),
        ),
      )
      .subscribe({
        next: ({ page, metadata }) => {
          this.items.set(page.items);
          this.total.set(page.total);
          this.metadata.set(metadata);
        },
        error: (e) => this.error.set(`Failed to load the items: ${message(e)}`),
      });
  }

  onPage(event: PageEvent) {
    const moved =
      event.pageIndex !== this.pageIndex() || event.pageSize !== this.pageSize();
    this.pageIndex.set(event.pageIndex);
    this.pageSize.set(event.pageSize);
    // The paginator also emits when it clamps itself (e.g. after the row count shrinks);
    // reloading for those would fetch the same window twice.
    if (moved) {
      this.reload.next();
    }
  }

  statusOf(id: number): ItemStatus {
    return this.metadata()[String(id)]?.status ?? 'draft';
  }

  /** True while the item holds changes that have not been published. */
  hasDraft(id: number): boolean {
    return this.metadata()[String(id)]?.has_draft ?? false;
  }

  /** Who published the item, or an empty string while nobody has. */
  publishedBy(id: number): string {
    return this.metadata()[String(id)]?.published_by?.username ?? '';
  }

  /** When the item's values were last saved, or a dash when that was never recorded. */
  updatedAt(id: number): string {
    const updated = this.metadata()[String(id)]?.updated_at;
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
        this.metadata.set({ ...this.metadata(), [String(id)]: metadata });
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
        this.stepBackIfPageIsGone();
      },
      error: (e) => this.error.set(`Delete failed: ${message(e)}`),
    });
  }

  /**
   * Reload, falling back a page when the deletion emptied the last one.
   *
   * Without this, deleting the only row of the final page would leave the table empty with
   * content still on the previous page.
   */
  private stepBackIfPageIsGone() {
    const remaining = Math.max(0, this.total() - 1);
    const lastPage = Math.max(0, Math.ceil(remaining / this.pageSize()) - 1);
    if (this.pageIndex() > lastPage) {
      this.pageIndex.set(lastPage);
    }
    this.reload.next();
  }
}
