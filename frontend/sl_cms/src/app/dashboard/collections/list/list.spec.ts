import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router, provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';
import { Observable, Subject, of, throwError } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { ItemMetadata, ItemMetadataMap } from 'app/models/item-status';
import { CollectionItemEntry, CollectionItemPage } from 'app/models/values/collection';
import { CollectionsService } from 'app/services/schema/collections.service';

import { TypedFixture } from 'app/core/testing/fixture';
import { CollectionItemList } from './list';
import { formatDateTime } from 'app/core/i18n/date-format';

/**
 * A fake collection holding as many items as a test needs. Paging is what is under test
 * here, so the stub really slices its list instead of always answering the same way.
 */
class StubCollectionsService {
  public published: number[] = [];
  public unpublished: number[] = [];
  public deleted: number[] = [];
  public metadata: ItemMetadataMap = {};
  /** Everything the fake server holds, in id order. */
  public all: CollectionItemEntry[] = [[1, { title: 'Hello' }]];
  /** When set, the next page request fails once, and the one after it works again. */
  public failNextPage: unknown = null;
  /** When set, a batch waits for the test to complete it. */
  public heldBatch?: Subject<unknown>;
  /** The windows the component asked for, in order. */
  public requested: { limit: number; offset: number }[] = [];

  /** What the schema editor saved for this collection. A test shapes it where it matters. */
  public schema: unknown = [
    { name: 'title', field_type: 'Text', required: false, width: 12, height: 1 },
  ];

  getCollectionSchema(): Observable<unknown> {
    return of(this.schema);
  }

  listCollectionItemsPage(
    _name: string,
    page: { limit: number; offset: number },
  ): Observable<CollectionItemPage> {
    this.requested.push(page);
    if (this.failNextPage) {
      const failure = this.failNextPage;
      this.failNextPage = null;
      return throwError(() => failure);
    }
    return of({
      items: this.all.slice(page.offset, page.offset + page.limit),
      total: this.all.length,
    });
  }

  listItemMetadata(): Observable<ItemMetadataMap> {
    return of(this.metadata);
  }

  publishItem(_name: string, id: number): Observable<ItemMetadata> {
    this.published.push(id);
    return of(metadata({ status: 'published', published_at: '2024-01-01T00:00:00Z' }));
  }

  unpublishItem(_name: string, id: number): Observable<ItemMetadata> {
    this.unpublished.push(id);
    return of(metadata({ status: 'draft', published_at: null }));
  }

  /** Batches and duplicates the screen asked for, so their effect can be read back. */
  public batches: { ids: number[]; status: string }[] = [];
  public duplicated: number[] = [];
  /** Ids in the batch that the server should refuse, as it does for one that is gone. */
  public refuse: number[] = [];

  setItemsStatus(_name: string, ids: number[], status: 'draft' | 'published'): Observable<unknown> {
    this.batches.push({ ids, status });
    if (this.heldBatch) {
      return this.heldBatch;
    }
    return of(
      ids.map((id) =>
        this.refuse.includes(id)
          ? { outcome: 'refused', id, code: 'not_found', message: `no item ${id}` }
          : {
              outcome: 'changed',
              id,
              metadata: metadata({
                status,
                published_at: status === 'published' ? '2024-01-01T00:00:00Z' : null,
              }),
            },
      ),
    );
  }

  duplicateItem(_name: string, id: number): Observable<number> {
    this.duplicated.push(id);
    return of(99);
  }

  deleteCollectionItem(_name: string, id: number): Observable<void> {
    this.deleted.push(id);
    this.all = this.all.filter(([itemId]) => itemId !== id);
    return of(void 0);
  }
}

/** Metadata with the timestamps filled in; only the status and time matter in these tests. */
function metadata(overrides: Partial<ItemMetadata>): ItemMetadata {
  return {
    status: 'draft',
    published_at: null,
    last_published_at: null,
    published_by: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    has_draft: false,
    ...overrides,
  };
}

function items(count: number): CollectionItemEntry[] {
  return Array.from({ length: count }, (_, index) => [index + 1, { title: `Item ${index}` }]);
}

/** Rows rendered in the table body. */
function rows(fixture: TypedFixture<CollectionItemList>): number {
  return fixture.nativeElement.querySelectorAll('tbody tr').length;
}

/** Permissions are the server's business; the screens are only told what to offer. */
function stubAuth(canEdit = true, canPublish = true, isAdmin = true) {
  return {
    provide: AuthService,
    useValue: {
      user: () => null,
      canEdit: () => canEdit,
      canPublish: () => canPublish,
      // The screens ask about the resource they are showing; these stubs answer the same way
      // everywhere.
      canEditIn: () => canEdit,
      canPublishIn: () => canPublish,
      isAdmin: () => isAdmin,
    },
  };
}

describe('CollectionItemList', () => {
  let component: CollectionItemList;
  let fixture: TypedFixture<CollectionItemList>;
  let stub: StubCollectionsService;
  let route: ReturnType<typeof stubActivatedRoute>;

  beforeEach(async () => {
    stub = new StubCollectionsService();
    route = stubActivatedRoute({ name: 'blog' });
    await TestBed.configureTestingModule({
      imports: [CollectionItemList],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
        { provide: CollectionsService, useValue: stub },
        stubAuth(),
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(CollectionItemList);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  // The columns are the fields the schema marks, in schema order - which is how an editor chooses
  // what identifies an item, rather than reading every field a collection happens to have.
  it('shows the columns the schema asks for', async () => {
    stub.schema = [
      {
        name: 'title',
        field_type: 'Text',
        required: false,
        width: 12,
        height: 1,
        show_in_list: true,
      },
      { name: 'body', field_type: { Markdown: {} }, required: false, width: 12, height: 1 },
      {
        name: 'count',
        field_type: 'Number',
        required: false,
        width: 12,
        height: 1,
        show_in_list: true,
      },
    ];
    stub.all = [[1, { title: 'Hello', body: '# long', count: 3 }]];
    // Another collection, so the schema above is what this screen loads.
    route.navigate({ name: 'posts' });
    await fixture.whenStable();
    fixture.detectChanges();

    const headers = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('thead th'),
      (cell: HTMLElement) => cell.textContent?.trim(),
    );
    expect(headers).toEqual(['', 'ID', 'title', 'count', 'Status', 'Updated', '']);

    // The values of the columns it does show, and not the one it does not.
    const cells = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('tbody tr td'),
      (cell: HTMLElement) => cell.textContent?.trim(),
    );
    expect(cells).toContain('Hello');
    expect(cells).toContain('3');
    expect(cells).not.toContain('# long');
  });

  // Nothing marked means no field columns: id, state and when it changed are what every row has,
  // and a field the author did not choose is not a column anybody asked for.
  it('shows no field columns when the schema marks none', async () => {
    stub.schema = [
      { name: 'title', field_type: 'Text', required: false, width: 12, height: 1 },
      { name: 'count', field_type: 'Number', required: false, width: 12, height: 1 },
    ];
    stub.all = [[1, { title: 'Hello', count: 3 }]];
    route.navigate({ name: 'posts' });
    await fixture.whenStable();
    fixture.detectChanges();

    const headers = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('thead th'),
      (cell: HTMLElement) => cell.textContent?.trim(),
    );
    expect(headers).toEqual(['', 'ID', 'Status', 'Updated', '']);

    const cells = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('tbody tr td'),
      (cell: HTMLElement) => cell.textContent?.trim(),
    );
    expect(cells).not.toContain('Hello');
    expect(cells).not.toContain('3');
  });

  // An author who wonders where the fields went is one link from the screen that chooses them -
  // and an editor, who cannot open that screen, is not offered a link that would refuse them.
  it('points at the schema when no column is chosen', async () => {
    route.navigate({ name: 'posts' });
    await fixture.whenStable();
    fixture.detectChanges();

    const link = fixture.nativeElement.querySelector<HTMLAnchorElement>('.note a');
    expect(link?.getAttribute('href')).toBe(
      `/settings/collections/${component.collectionName()}/schema`,
    );
  });

  // An image column is a column of pictures: a url in a cell says which item this is to nobody.
  it('draws image columns as thumbnails', async () => {
    stub.schema = [
      {
        name: 'photo',
        field_type: 'Image',
        required: false,
        width: 12,
        height: 1,
        show_in_list: true,
      },
      {
        name: 'gallery',
        field_type: { Array: ['Image'] },
        required: false,
        width: 12,
        height: 1,
        show_in_list: true,
      },
    ];
    stub.all = [
      [
        1,
        {
          photo: { id: 3, url: '/api/images/logo.png' },
          gallery: [
            { id: 3, url: '/api/images/logo.png' },
            { id: 4, url: '/api/images/photo.png' },
          ],
        },
      ],
      // A value that was written but never read back carries a bare id, and there is no picture.
      [2, { photo: 5, gallery: [] }],
    ];
    route.navigate({ name: 'posts' });
    await fixture.whenStable();
    fixture.detectChanges();

    const thumbnails = fixture.nativeElement.querySelectorAll<HTMLImageElement>(
      'tbody tr:first-child td.value img',
    );
    expect(Array.from(thumbnails, (image: HTMLImageElement) => image.getAttribute('src'))).toEqual([
      '/api/images/logo.png',
      '/api/images/logo.png',
      '/api/images/photo.png',
    ]);

    const secondRow = fixture.nativeElement.querySelectorAll<HTMLElement>('tbody tr')[1];
    expect(secondRow.textContent).toContain('id 5');
  });

  // More images than a cell can show: the ones it draws, and how many are not drawn.
  it('counts the images a cell has no room for', async () => {
    stub.schema = [
      {
        name: 'gallery',
        field_type: { Array: ['Image'] },
        required: false,
        width: 12,
        height: 1,
        show_in_list: true,
      },
    ];
    stub.all = [
      [
        1,
        {
          gallery: [1, 2, 3, 4, 5].map((id) => ({ id, url: `/api/images/${id}.png` })),
        },
      ],
    ];
    route.navigate({ name: 'posts' });
    await fixture.whenStable();
    fixture.detectChanges();

    const images = fixture.nativeElement.querySelectorAll('tbody td.value img');
    expect(images.length).toBe(3);
    expect(fixture.nativeElement.querySelector('tbody td.value .more')?.textContent?.trim()).toBe(
      '+2',
    );
  });

  // A relation column names what it points at: the content itself is the delivery API's business.
  it('names the content a relation column points at', async () => {
    stub.schema = [
      {
        name: 'author',
        field_type: { Relation: { target: { kind: 'collection', name: 'authors' } } },
        required: false,
        width: 12,
        height: 1,
        show_in_list: true,
      },
      {
        name: 'landing',
        field_type: { Relation: { target: { kind: 'single_page', name: 'home' } } },
        required: false,
        width: 12,
        height: 1,
        show_in_list: true,
      },
    ];
    stub.all = [[1, { author: [{ target: 'authors', item: 7 }], landing: [{ target: 'home' }] }]];
    route.navigate({ name: 'posts' });
    await fixture.whenStable();
    fixture.detectChanges();

    const cells = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('tbody td.value'),
      (cell: HTMLElement) => cell.textContent?.trim(),
    );
    expect(cells).toEqual(['authors #7', 'home']);
  });

  // The sidebar switches collections without leaving this route, so the rows and the pager have
  // to be asked for again rather than left as the previous collection's.
  it('loads another collection when the parameter changes', async () => {
    component.pageIndex.set(1);
    stub.requested.length = 0;

    route.navigate({ name: 'pages' });
    await fixture.whenStable();

    expect(component.collectionName()).toBe('pages');
    expect(component.pageIndex()).toBe(0);
    expect(stub.requested.length).toBeGreaterThan(0);
  });

  // A failure that ends the subscription would leave the screen stuck: no page change, no delete
  // and no retry would fetch anything again.
  it('asks again after a load that failed', async () => {
    stub.failNextPage = { error: { code: 'internal', message: 'boom' } };

    component.onPage({ pageIndex: 1, pageSize: 25, length: 1 });
    await fixture.whenStable();
    fixture.detectChanges();
    expect(component.error()).not.toBeNull();

    const retry = fixture.nativeElement.querySelector('.error button') as HTMLButtonElement;
    expect(retry).toBeTruthy();
    retry.click();
    await fixture.whenStable();
    fixture.detectChanges();

    expect(component.error()).toBeNull();
    expect(rows(fixture)).toBeGreaterThan(0);
  });

  // Every collection numbers its items from one, so a selection that outlived a switch would name
  // whatever holds those ids in the collection now on screen.
  it('forgets the selection when the collection changes', async () => {
    component.toggleAll();
    expect(component.selected().size).toBe(1);

    route.navigate({ name: 'pages' });
    await fixture.whenStable();

    expect(component.selected().size).toBe(0);
  });

  // The other half: a batch already on its way answers about the rows that were picked, which are
  // in the collection the reader has left.
  it('leaves the collection on screen alone when a batch answers late', async () => {
    const held = new Subject<unknown>();
    stub.heldBatch = held;
    component.toggleAll();
    component.setSelectedPublished('published');

    route.navigate({ name: 'pages' });
    await fixture.whenStable();
    held.next([{ outcome: 'changed', id: 1, metadata: metadata({ status: 'published' }) }]);
    held.complete();
    await fixture.whenStable();
    fixture.detectChanges();

    expect(component.batchReport()).toBeNull();
    expect(component.metadata()['1']).toBeUndefined();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  /** The server omits never-published items, so the screen has to default to Draft. */
  it('shows an item with no stored status as a draft', () => {
    fixture.detectChanges();

    const badge = fixture.nativeElement.querySelector('app-item-status .badge') as HTMLElement;
    expect(badge.textContent?.trim()).toBe('Draft');
    expect(component.statusOf(1)).toBe('draft');
  });

  it('publishes an item from the list and shows the new state', () => {
    fixture.detectChanges();

    const button = fixture.nativeElement.querySelector(
      'button[aria-label="publish item 1"]',
    ) as HTMLButtonElement;
    button.click();
    fixture.detectChanges();

    expect(stub.published).toEqual([1]);
    expect(component.statusOf(1)).toBe('published');
    const badge = fixture.nativeElement.querySelector('app-item-status .badge') as HTMLElement;
    expect(badge.textContent?.trim()).toBe('Published');
  });

  it('hides a published item again', () => {
    stub.metadata = { '1': metadata({ status: 'published' }) };
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    const button = fresh.nativeElement.querySelector(
      'button[aria-label="unpublish item 1"]',
    ) as HTMLButtonElement;
    button.click();
    fresh.detectChanges();

    expect(stub.unpublished).toEqual([1]);
    expect(fresh.componentInstance.statusOf(1)).toBe('draft');
  });

  /** The audit trail: the list says who published the row, and stays quiet for drafts. */
  it('shows who published an item, and nothing for a draft', () => {
    stub.metadata = {
      '1': metadata({
        status: 'published',
        published_at: '2024-05-06T07:08:09Z',
        published_by: { id: 'u1', username: 'publisher@example.com' },
      }),
    };
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    const note = fresh.nativeElement.querySelector('.publisher') as HTMLElement;
    expect(note.textContent?.trim()).toBe('publisher@example.com');

    // A draft has no publisher to show, even though it was published before.
    stub.metadata = { '1': metadata({ status: 'draft', published_by: null }) };
    const other: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    other.detectChanges();
    expect(other.nativeElement.querySelector('.publisher')).toBeNull();
  });

  it('shows when each item was last saved', () => {
    stub.metadata = { '1': metadata({ updated_at: '2024-05-06T07:08:09Z', has_draft: false }) };
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    // The wording is locale dependent, so compare against the same formatting.
    const cell = fresh.nativeElement.querySelector('.updated') as HTMLElement;
    expect(cell.textContent?.trim()).toBe(formatDateTime('2024-05-06T07:08:09Z', 'en'));
  });

  it('marks an item whose changes are not published yet', () => {
    stub.metadata = { '1': metadata({ status: 'published', has_draft: true }) };
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    expect(fresh.componentInstance.hasDraft(1)).toBe(true);
    const note = fresh.nativeElement.querySelector('.draft-note') as HTMLElement;
    expect(note.textContent).toContain('Unsaved changes');
  });

  /** Pending changes on a published row can be released without taking it down first. */
  it('releases the waiting changes of a published row', () => {
    stub.metadata = { '1': metadata({ status: 'published', has_draft: true }) };
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    const release = fresh.nativeElement.querySelector(
      'button[aria-label="publish the changes of item 1"]',
    ) as HTMLButtonElement;
    expect(release).toBeTruthy();
    release.click();
    fresh.detectChanges();

    expect(stub.published).toEqual([1]);
    expect(stub.unpublished).toEqual([]);
    // The row still offers to take the item down.
    expect(fresh.nativeElement.querySelector('button[aria-label="unpublish item 1"]')).toBeTruthy();
  });

  it('offers no release button when a published row has nothing waiting', () => {
    stub.metadata = { '1': metadata({ status: 'published' }) };
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    expect(
      fresh.nativeElement.querySelector('button[aria-label="publish the changes of item 1"]'),
    ).toBeNull();
    expect(fresh.nativeElement.querySelector('button[aria-label="unpublish item 1"]')).toBeTruthy();
  });

  /** Content saved before the CMS recorded timestamps has nothing to show. */
  it('shows a dash when the update time is unknown', () => {
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    expect(fresh.componentInstance.updatedAt(1)).toBe('—');
    expect((fresh.nativeElement.querySelector('.updated') as HTMLElement).textContent?.trim()).toBe(
      '—',
    );
  });

  it('asks for one page and shows how many items there are in total', () => {
    stub.all = items(60);
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    expect(stub.requested[0]).toEqual({ limit: 25, offset: 0 });
    expect(rows(fresh)).toBe(25);
    expect(fresh.componentInstance.total()).toBe(60);
    // The pager is what makes the rest of the collection reachable.
    expect(fresh.nativeElement.querySelector('mat-paginator')).toBeTruthy();
  });

  it('asks the server for the next window when the pager moves', () => {
    stub.all = items(60);
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    fresh.componentInstance.onPage({ pageIndex: 2, pageSize: 25, length: 60 });
    fresh.detectChanges();

    expect(stub.requested[stub.requested.length - 1]).toEqual({ limit: 25, offset: 50 });
    expect(rows(fresh)).toBe(10);
  });

  it('honours a different page size', () => {
    stub.all = items(60);
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    fresh.componentInstance.onPage({ pageIndex: 0, pageSize: 10, length: 60 });
    fresh.detectChanges();

    expect(stub.requested[stub.requested.length - 1]).toEqual({ limit: 10, offset: 0 });
    expect(rows(fresh)).toBe(10);
  });

  it('steps back a page when the last row of the final page is deleted', () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    stub.all = items(26);
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    fresh.componentInstance.onPage({ pageIndex: 1, pageSize: 25, length: 26 });
    fresh.detectChanges();
    expect(rows(fresh)).toBe(1);

    // That single row goes away, so page 1 no longer exists.
    fresh.componentInstance.delete(26);
    fresh.detectChanges();

    expect(stub.deleted).toEqual([26]);
    expect(stub.requested[stub.requested.length - 1]).toEqual({ limit: 25, offset: 0 });
    expect(rows(fresh)).toBe(25);
  });

  // Selecting rows is how a batch is asked for; the answer is per item, so a refusal is
  // reported rather than swallowed.
  it('publishes and unpublishes everything selected, and reports what it could not', () => {
    stub.all = [
      [1, { title: 'one' }],
      [2, { title: 'two' }],
    ];
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();
    const component = fresh.componentInstance;
    component.toggleAll();
    expect([...component.selected()]).toEqual([1, 2]);

    component.setSelectedPublished('published');
    expect(stub.batches).toEqual([{ ids: [1, 2], status: 'published' }]);
    expect(component.selected().size).toBe(0);
    expect(component.batchReport()).toEqual({ changed: 2, refused: 0 });
    expect(component.statusOf(1)).toBe('published');

    // One of them is gone by the time the batch runs.
    stub.refuse = [2];
    component.toggleAll();
    component.setSelectedPublished('draft');
    expect(component.batchReport()).toEqual({ changed: 1, refused: 1 });
    expect(component.refusals()).toEqual([{ id: 2, code: 'not_found', message: 'no item 2' }]);
  });

  // Duplicating is only useful if the next act is editing the copy.
  it('copies an item and opens the copy', () => {
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();
    const router = TestBed.inject(Router);
    const navigate = vi.spyOn(router, 'navigate').mockResolvedValue(true);

    fresh.componentInstance.duplicate(1);

    expect(stub.duplicated).toEqual([1]);
    expect(navigate).toHaveBeenCalledWith(['/collections', 'blog', 'edit', 99]);
  });

  it('stays on the same page when the deletion leaves it populated', () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    stub.all = items(60);
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    fresh.componentInstance.delete(1);
    fresh.detectChanges();

    expect(stub.deleted).toEqual([1]);
    expect(stub.requested[stub.requested.length - 1]).toEqual({ limit: 25, offset: 0 });
    expect(rows(fresh)).toBe(25);
  });
});

describe('CollectionItemList (read-only account)', () => {
  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [CollectionItemList],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        stubAuth(false, false, false),
        { provide: CollectionsService, useValue: new StubCollectionsService() },
      ],
    }).compileComponents();
  });

  it('offers nothing the server would refuse', () => {
    const fresh: TypedFixture<CollectionItemList> = TestBed.createComponent(CollectionItemList);
    fresh.detectChanges();

    expect(fresh.nativeElement.querySelector('button[aria-label^="publish item"]')).toBeNull();
    expect(fresh.nativeElement.querySelector('.note')).toBeNull();
    expect(fresh.nativeElement.querySelector('button[aria-label^="delete item"]')).toBeNull();
    expect(fresh.nativeElement.textContent).not.toContain('New item');
    // The rows themselves are still readable.
    expect(fresh.nativeElement.querySelectorAll('tbody tr').length).toBe(1);
  });
});
