import { Component, computed, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { BehaviorSubject, forkJoin, switchMap } from 'rxjs';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatPaginatorModule, PageEvent } from '@angular/material/paginator';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';

import { AuthService } from 'app/core/auth/auth.service';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe, failure } from 'app/core/i18n/message';
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
  imports: [ MatTooltipModule,
    ItemStatusBadge,
    MatButtonModule,
    MatIconModule,
    MatPaginatorModule,
    MessagePipe,
    RouterLink,
    TranslocoPipe,
  ],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class List {
  private route = inject(ActivatedRoute);
  private collectionsService = inject(CollectionsService);
  private i18n = inject(TranslocoService);
  private dates = inject(DateTimeFormat);
  /** What the signed-in account may do; the server enforces the same rules. */
  public auth = inject(AuthService);

  /**
   * The collection on screen.
   *
   * A signal, read from the parameter stream: the sidebar switches collections without leaving
   * this route, and the router reuses the component, so a parameter read once would leave the
   * previous collection's items on screen.
   */
  public collectionName = signal('');
  public schema = signal<CollectionSchema>([]);
  /**
   * The rows on screen. A signal, not a plain field: the response can land while Angular
   * is checking the view, and a plain field changing then trips
   * `ExpressionChangedAfterItHasBeenCheckedError` — which in dev mode left the row's
   * bindings unapplied (empty cells) when the last page went from 25 rows to one.
   */
  public items = signal<CollectionItemEntry[]>([]);
  /** The failure to show, as a key or as the server's own words. */
  public error = signal<Message | null>(null);
  /** Draft/published state per item id; the server sends drafts for untouched items. */
  public metadata = signal<ItemMetadataMap>({});

  /** What this account may do *with this collection*, overrides included. */
  public canEdit = computed(() => this.auth.canEditIn('collections', this.collectionName()));
  public canPublish = computed(() => this.auth.canPublishIn('collections', this.collectionName()));
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
    // Rows, their status and the total are fetched together: the table and the pager both
    // need them, and a collection can be too long to send in one response.
    this.reload
      .pipe(
        switchMap(() =>
          forkJoin({
            page: this.collectionsService.listCollectionItemsPage(this.collectionName(), {
              limit: this.pageSize(),
              offset: this.pageIndex() * this.pageSize(),
            }),
            metadata: this.collectionsService.listItemMetadata(this.collectionName()),
          }),
        ),
        takeUntilDestroyed(),
      )
      .subscribe({
        next: ({ page, metadata }) => {
          this.items.set(page.items);
          this.total.set(page.total);
          this.metadata.set(metadata);
        },
        error: (e) => this.error.set(failure('content.failedToLoadItems', e)),
      });

    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const name = params.get('name') ?? '';
      if (name !== this.collectionName()) {
        this.load(name);
      }
    });
  }

  /** Everything on screen belongs to one collection, so a switch starts from nothing. */
  private load(name: string) {
    this.collectionName.set(name);
    this.schema.set([]);
    this.items.set([]);
    this.total.set(0);
    this.metadata.set({});
    this.error.set(null);
    this.pageIndex.set(0);

    this.collectionsService.getCollectionSchema(name).subscribe({
      next: (schema) => this.schema.set(schema),
      error: (e) => this.error.set(failure('content.failedToLoadSchema', e)),
    });
    this.reload.next();
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
    return this.dates.format(this.metadata()[String(id)]?.updated_at);
  }

  /** Publish or unpublish one item, without leaving the list. */
  /** Publish a draft, or release the changes waiting on a published item. */
  publish(id: number) {
    this.setPublished(id, true);
  }

  unpublish(id: number) {
    this.setPublished(id, false);
  }

  private setPublished(id: number, published: boolean) {
    const request = published
      ? this.collectionsService.publishItem(this.collectionName(), id)
      : this.collectionsService.unpublishItem(this.collectionName(), id);

    request.subscribe({
      next: (metadata) => {
        this.error.set(null);
        this.metadata.set({ ...this.metadata(), [String(id)]: metadata });
      },
      error: (e) => this.error.set(failure('content.failedToChangePublished', e)),
    });
  }

  delete(id: number) {
    if (!confirm(this.i18n.translate('content.deleteItemConfirm', { id }))) {
      return;
    }
    this.collectionsService.deleteCollectionItem(this.collectionName(), id).subscribe({
      next: () => {
        this.error.set(null);
        this.stepBackIfPageIsGone();
      },
      error: (e) => this.error.set(failure('content.deleteFailed', e)),
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
