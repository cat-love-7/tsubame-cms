import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { PageEvent } from '@angular/material/paginator';
import { provideRouter } from '@angular/router';
import { Observable, of } from 'rxjs';

import { ItemMetadata, ItemMetadataMap } from 'app/models/item-status';
import { CollectionItemEntry, CollectionItemPage } from 'app/models/values/collection';
import { CollectionsService } from 'app/services/schema/collections.service';

import { List } from './list';

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
  /** The windows the component asked for, in order. */
  public requested: { limit: number; offset: number }[] = [];

  getCollectionSchema(): Observable<unknown> {
    return of([{ name: 'title', field_type: 'Text', required: false, width: 12, height: 1 }]);
  }

  listCollectionItemsPage(
    _name: string,
    page: { limit: number; offset: number },
  ): Observable<CollectionItemPage> {
    this.requested.push(page);
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
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    ...overrides,
  };
}

function items(count: number): CollectionItemEntry[] {
  return Array.from({ length: count }, (_, index) => [index + 1, { title: `Item ${index}` }]);
}

/** Rows rendered in the table body. */
function rows(fixture: ComponentFixture<List>): number {
  return fixture.nativeElement.querySelectorAll('tbody tr').length;
}

describe('List', () => {
  let component: List;
  let fixture: ComponentFixture<List>;
  let stub: StubCollectionsService;

  beforeEach(async () => {
    stub = new StubCollectionsService();
    await TestBed.configureTestingModule({
      imports: [List],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: CollectionsService, useValue: stub },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(List);
    component = fixture.componentInstance;
    await fixture.whenStable();
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
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    const button = fresh.nativeElement.querySelector(
      'button[aria-label="unpublish item 1"]',
    ) as HTMLButtonElement;
    button.click();
    fresh.detectChanges();

    expect(stub.unpublished).toEqual([1]);
    expect(fresh.componentInstance.statusOf(1)).toBe('draft');
  });

  it('shows when each item was last saved', () => {
    stub.metadata = { '1': metadata({ updated_at: '2024-05-06T07:08:09Z' }) };
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    // The wording is locale dependent, so compare against the same formatting.
    const cell = fresh.nativeElement.querySelector('.updated') as HTMLElement;
    expect(cell.textContent?.trim()).toBe(new Date('2024-05-06T07:08:09Z').toLocaleString());
  });

  /** Content saved before the CMS recorded timestamps has nothing to show. */
  it('shows a dash when the update time is unknown', () => {
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    expect(fresh.componentInstance.updatedAt(1)).toBe('—');
    expect((fresh.nativeElement.querySelector('.updated') as HTMLElement).textContent?.trim()).toBe(
      '—',
    );
  });

  it('asks for one page and shows how many items there are in total', () => {
    stub.all = items(60);
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    expect(stub.requested[0]).toEqual({ limit: 25, offset: 0 });
    expect(rows(fresh)).toBe(25);
    expect(fresh.componentInstance.total()).toBe(60);
    // The pager is what makes the rest of the collection reachable.
    expect(fresh.nativeElement.querySelector('mat-paginator')).toBeTruthy();
  });

  it('asks the server for the next window when the pager moves', () => {
    stub.all = items(60);
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    fresh.componentInstance.onPage({ pageIndex: 2, pageSize: 25, length: 60 } as PageEvent);
    fresh.detectChanges();

    expect(stub.requested[stub.requested.length - 1]).toEqual({ limit: 25, offset: 50 });
    expect(rows(fresh)).toBe(10);
  });

  it('honours a different page size', () => {
    stub.all = items(60);
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    fresh.componentInstance.onPage({ pageIndex: 0, pageSize: 10, length: 60 } as PageEvent);
    fresh.detectChanges();

    expect(stub.requested[stub.requested.length - 1]).toEqual({ limit: 10, offset: 0 });
    expect(rows(fresh)).toBe(10);
  });

  it('steps back a page when the last row of the final page is deleted', () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    stub.all = items(26);
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    fresh.componentInstance.onPage({ pageIndex: 1, pageSize: 25, length: 26 } as PageEvent);
    fresh.detectChanges();
    expect(rows(fresh)).toBe(1);

    // That single row goes away, so page 1 no longer exists.
    fresh.componentInstance.delete(26);
    fresh.detectChanges();

    expect(stub.deleted).toEqual([26]);
    expect(stub.requested[stub.requested.length - 1]).toEqual({ limit: 25, offset: 0 });
    expect(rows(fresh)).toBe(25);
  });

  it('stays on the same page when the deletion leaves it populated', () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    stub.all = items(60);
    const fresh = TestBed.createComponent(List);
    fresh.detectChanges();

    fresh.componentInstance.delete(1);
    fresh.detectChanges();

    expect(stub.deleted).toEqual([1]);
    expect(stub.requested[stub.requested.length - 1]).toEqual({ limit: 25, offset: 0 });
    expect(rows(fresh)).toBe(25);
  });
});
