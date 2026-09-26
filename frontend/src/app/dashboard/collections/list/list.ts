import { Component, DestroyRef, computed, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { BehaviorSubject, catchError, forkJoin, of, switchMap } from 'rxjs';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatPaginatorModule, PageEvent } from '@angular/material/paginator';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';

import { AuthService } from 'app/core/auth/auth.service';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe, failure } from 'app/core/i18n/message';
import { ItemMetadataMap, ItemStatus } from 'app/models/item-status';
import { apiUrl } from 'app/core/api-url';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema, isArrayFieldSchema, relationOptionsOf } from 'app/models/schema/fields';
import { CollectionItemEntry } from 'app/models/values/collection';
import {
  formatFieldValue,
  imagesOf,
  referenceName,
  relationRefsOf,
} from 'app/models/values/fields';
import { RelationLabelsService } from 'app/services/relations/relation-labels.service';
import { CollectionsService } from 'app/services/schema/collections.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';

/** Rows per page. Small enough to read, large enough to scan. */
const DEFAULT_PAGE_SIZE = 25;

@Component({
  selector: 'app-collection-item-list',
  imports: [
    MatTooltipModule,
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
export class CollectionItemList {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  /** When this screen goes away, so does everything it still has in flight. */
  private destroyRef = inject(DestroyRef);
  private collectionsService = inject(CollectionsService);
  private labelsService = inject(RelationLabelsService);
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

  /**
   * The columns the schema asks for, in schema order.
   *
   * A list is what an editor scans to find one item, and a schema of a dozen fields makes a table
   * nobody can read - so the schema says which fields identify an item, and those are the columns.
   * A schema that marks none gets none: id, state and when it changed are what every row has, and
   * showing a field the author did not choose would be answering a question they did not ask.
   */
  public listColumns = computed(() => this.schema().filter((field) => field.show_in_list));

  /**
   * The table's rows, with each cell already worked out.
   *
   * The cells are built here rather than in the template so a column is read once per row: an image
   * column needs the value twice over (which images, and how many there were) and calling that per
   * binding would walk the same value several times per cell.
   */
  public rows = computed(() =>
    this.items().map(([id, values]) => ({
      id,
      values,
      cells: this.listColumns().map((field) => ({
        name: field.name,
        text: cellText(field, values[field.name], this.labels()),
        images: isImageColumn(field) ? imagesOf(values[field.name]) : [],
      })),
    })),
  );

  /**
   * What the referenced items are called, by reference.
   *
   * A relation column shows the title its target's schema names rather than `categories #3`: the
   * point of a reference in a list is which item it is, and an id is not that.
   */
  public labels = signal<ReadonlyMap<string, string>>(new Map());

  /** How many thumbnails one cell draws before it says how many more there are. */
  public readonly maxThumbnails = 3;

  /** Where an uploaded image is served from, for the thumbnails. */
  public imageUrl = apiUrl;

  /** What this account may do *with this collection*, overrides included. */
  public canEdit = computed(() => this.auth.canEditIn('collections', this.collectionName()));
  public canPublish = computed(() => this.auth.canPublishIn('collections', this.collectionName()));
  /** Items in the collection, not just on this page. Drives the paginator. */
  public total = signal(0);
  public pageIndex = signal(0);
  /**
   * The order the list is read in, spelled the way the API spells it: newest first by default,
   * which is what the service answers when nobody asks for another key.
   */
  public sortChoice = signal('-id');

  public pageSize = signal(DEFAULT_PAGE_SIZE);
  public readonly pageSizeOptions = [10, 25, 50, 100];

  /**
   * Order by a key, the other way round when it is already the key.
   *
   * One click asks for a column ascending, a second for it descending; the arrow in the header
   * says which, and where values are equal the id decides so the pages stay put.
   */
  public setSort(key: string) {
    const current = this.sortChoice();
    this.sortChoice.set(current === key ? `-${key}` : key);
    this.pageIndex.set(0);
    this.reload.next();
  }

  /** Whether this column is the one the list is ordered by, and which way. */
  public sortDirection(key: string): 'asc' | 'desc' | null {
    const current = this.sortChoice();
    if (current === key) {
      return 'asc';
    }
    return current === `-${key}` ? 'desc' : null;
  }

  /** The same, as the attribute a screen reader reads on the header. */
  public ariaSort(key: string): 'ascending' | 'descending' | 'none' {
    switch (this.sortDirection(key)) {
      case 'asc':
        return 'ascending';
      case 'desc':
        return 'descending';
      default:
        return 'none';
    }
  }
  /** Exposed for the template. */
  public format = formatFieldValue;

  /**
   * The rows the batch actions apply to.
   *
   * A set of ids rather than rows: a reload replaces the row objects, and a selection should
   * survive that.
   */
  public selected = signal<ReadonlySet<number>>(new Set());
  /** Rows that could not be changed in the last batch, so the report can name them. */
  public refusals = signal<{ id: number; code: string; message: string }[]>([]);

  /** Re-issues the page request after a delete or a page change. */
  private reload = new BehaviorSubject<void>(undefined);

  /**
   * Which visit to a collection the answers on screen belong to.
   *
   * Switching collections reuses this component, so a slow answer for the one before - the schema
   * most of all - would otherwise land on the one now named, and the next save would write it
   * there. The page request cancels itself (`switchMap`), the schema request cannot, so it is
   * answered by comparing this.
   */
  private loadToken = 0;

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
              sort: this.sortChoice(),
            }),
            metadata: this.collectionsService.listItemMetadata(this.collectionName()),
          }).pipe(
            // Handled *inside* the switch: an error that reaches the outer subscription ends it,
            // and from then on no page change, delete or retry would fetch anything again - the
            // screen would be stuck on whatever it happened to show.
            catchError((e) => {
              this.error.set(failure('content.failedToLoadItems', e));
              return of(null);
            }),
          ),
        ),
        takeUntilDestroyed(),
      )
      .subscribe((loaded) => {
        if (loaded === null) {
          return;
        }
        this.error.set(null);
        this.items.set(loaded.page.items);
        this.total.set(loaded.page.total);
        this.metadata.set(loaded.metadata);
        this.loadLabels(loaded.page.items);
      });

    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const name = params.get('name') ?? '';
      if (name !== this.collectionName()) {
        this.load(name);
      }
    });
  }

  /**
   * Ask what the rows' references are called.
   *
   * Only the relation columns, and only what is on screen: a list page asks about the items it
   * shows, not about a collection.
   */
  private loadLabels(items: CollectionItemEntry[]) {
    const relations = this.listColumns().filter(
      (field) => relationOptionsOf(field.field_type).length > 0,
    );
    if (relations.length === 0) {
      this.labels.set(new Map());
      return;
    }
    const references = items.flatMap(([, values]) =>
      relations.flatMap((field) => relationRefsOf(values[field.name])),
    );
    if (references.length === 0) {
      this.labels.set(new Map());
      return;
    }
    this.labelsService
      .labelsFor(references)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (labels) => this.labels.set(labels),
        // A name that cannot be fetched is a name the row shows as the reference itself; the list
        // is still readable, so this is not worth an error banner.
        error: () => this.labels.set(new Map()),
      });
  }

  /** Everything on screen belongs to one collection, so a switch starts from nothing. */
  private load(name: string) {
    const token = ++this.loadToken;
    this.collectionName.set(name);
    this.schema.set([]);
    this.items.set([]);
    this.total.set(0);
    this.metadata.set({});
    this.error.set(null);
    this.pageIndex.set(0);
    // The selection is a set of ids, and every collection numbers its items from one: a batch
    // pressed after a switch would otherwise publish whatever holds those ids *here*.
    this.selected.set(new Set());
    this.refusals.set([]);
    this.batchReport.set(null);

    this.collectionsService
      .getCollectionSchema(name)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (schema) => {
          if (token === this.loadToken) {
            this.schema.set(schema);
          }
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.failedToLoadSchema', e));
          }
        },
      });
    this.reload.next();
  }

  /** Ask again after a failure. The pager does this for a page change; this is for when there is
   *  nothing on screen to click. */
  retry() {
    this.load(this.collectionName());
  }

  onPage(event: PageEvent) {
    const moved = event.pageIndex !== this.pageIndex() || event.pageSize !== this.pageSize();
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

  /** Whether every row on screen is selected, which is what the header checkbox asks. */
  public allSelected = computed(
    () => this.items().length > 0 && this.items().every(([id]) => this.selected().has(id)),
  );

  toggleSelected(id: number) {
    const next = new Set(this.selected());
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    this.selected.set(next);
  }

  toggleAll() {
    this.selected.set(this.allSelected() ? new Set() : new Set(this.items().map(([id]) => id)));
  }

  /**
   * Publish or unpublish everything selected, and say what happened.
   *
   * Per item, because the server answers per item: a batch that reported only success would hide
   * the ones that were refused, and the editor would believe the site changed when it did not.
   */
  setSelectedPublished(status: ItemStatus) {
    const ids = [...this.selected()];
    if (ids.length === 0) {
      return;
    }
    // Publishing and unpublishing change what the site serves, so the answer to "are you sure?" is
    // the editor's - the same as deleting one item (`content.publishSelectedConfirm`).
    const question =
      status === 'published'
        ? 'content.publishSelectedConfirm'
        : 'content.unpublishSelectedConfirm';
    if (!confirm(this.i18n.translate(question, { count: ids.length }))) {
      return;
    }
    // The collection the rows were picked in, not the one on screen when the answer lands: the ids
    // are only meaningful there.
    const started = { name: this.collectionName(), generation: this.loadToken };
    this.collectionsService
      .setItemsStatus(started.name, ids, status)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (outcomes) => {
          if (!this.stillOn(started)) {
            return;
          }
          this.error.set(null);
          const refusals: { id: number; code: string; message: string }[] = [];
          // The server answered with the new metadata, so the badges move without a reload.
          const metadata = { ...this.metadata() };
          let changed = 0;
          for (const outcome of outcomes) {
            if (outcome.outcome === 'changed') {
              changed += 1;
              metadata[String(outcome.id)] = outcome.metadata;
            } else {
              refusals.push({ id: outcome.id, code: outcome.code, message: outcome.message });
            }
          }
          this.refusals.set(refusals);
          this.selected.set(new Set());
          this.metadata.set(metadata);
          this.batchReport.set({ changed, refused: refusals.length });
        },
        error: (e) => {
          if (this.stillOn(started)) {
            this.error.set(failure('content.failedToChangePublished', e));
          }
        },
      });
  }

  /** Whether the screen is still on the collection a slow answer was about, as it was then. */
  private stillOn(started: { name: string; generation: number }): boolean {
    return this.collectionName() === started.name && this.loadToken === started.generation;
  }

  /** What the last batch did, so the screen can say it (and not only when something failed). */
  public batchReport = signal<{ changed: number; refused: number } | null>(null);

  /** Publish or unpublish one item, without leaving the list. */
  /** Publish a draft, or release the changes waiting on a published item. */
  publish(id: number) {
    this.setPublished(id, true);
  }

  unpublish(id: number) {
    this.setPublished(id, false);
  }

  private setPublished(id: number, published: boolean) {
    // The same confirmation a delete asks for: this is what the site serves, not what the editor
    // is looking at.
    const question = published ? 'content.publishItemConfirm' : 'content.unpublishItemConfirm';
    if (!confirm(this.i18n.translate(question, { id }))) {
      return;
    }
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

  /**
   * Copy an item and open the copy, after asking.
   *
   * The copy is a draft with its unique fields empty (the server's rule), so the editor lands in a
   * form that only needs the parts which have to be new. It also *leaves the list*, and the button
   * sits next to "edit" with an icon that reads the same at a glance - so a mis-click used to cost
   * the reader their place in the list and an item they did not ask for. Every other button that
   * changes what the site serves asks first (delete, publish, unpublish, discard); this one now
   * does too, in the same shape.
   */
  duplicate(id: number) {
    if (!confirm(this.i18n.translate('content.duplicateItemConfirm', { id }))) {
      return;
    }
    this.collectionsService.duplicateItem(this.collectionName(), id).subscribe({
      next: (created) => {
        this.error.set(null);
        void this.router.navigate(['/collections', this.collectionName(), 'edit', created]);
      },
      error: (e) => this.error.set(failure('content.duplicateFailed', e)),
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

/**
 * Whether this column holds images, which a cell shows as thumbnails rather than as urls.
 *
 * An image array is the same column with several in it; anything else - a relation, a composite, a
 * text - is read as its one-line rendering.
 */
function isImageColumn(field: FieldSchema): boolean {
  const type = field.field_type;
  return type === 'Image' || (isArrayFieldSchema(type) && type.Array.includes('Image'));
}

/**
 * What one cell says.
 *
 * A relation column is the item it points at, named the way the target's schema names it; anything
 * else is its one-line rendering.
 */
function cellText(field: FieldSchema, value: unknown, labels: ReadonlyMap<string, string>): string {
  if (relationOptionsOf(field.field_type).length === 0) {
    return formatFieldValue(value as never);
  }
  const references = relationRefsOf(value as never);
  return references.length > 0
    ? references.map((reference) => referenceName(reference, labels)).join(', ')
    : '—';
}
