import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { Observable, of } from 'rxjs';

import { ItemMetadata, ItemMetadataMap } from 'app/models/item-status';
import { CollectionsService } from 'app/services/schema/collections.service';

import { List } from './list';

/**
 * The part of `CollectionsService` this screen uses. Status handling is what is under
 * test here, not HTTP, so the stub answers synchronously.
 */
class StubCollectionsService {
  public published: number[] = [];
  public unpublished: number[] = [];
  public metadata: ItemMetadataMap = {};

  getCollectionSchema(): Observable<unknown> {
    return of([{ name: 'title', field_type: 'Text', required: false, width: 12, height: 1 }]);
  }

  listCollectionItems(): Observable<unknown> {
    return of([[1, { title: 'Hello' }]]);
  }

  listItemMetadata(): Observable<ItemMetadataMap> {
    return of(this.metadata);
  }

  publishItem(_name: string, id: number): Observable<ItemMetadata> {
    this.published.push(id);
    return of({ status: 'published', published_at: '2024-01-01T00:00:00Z', created_at: '2024-01-01T00:00:00Z', updated_at: '2024-01-01T00:00:00Z' });
  }

  unpublishItem(_name: string, id: number): Observable<ItemMetadata> {
    this.unpublished.push(id);
    return of({ status: 'draft', published_at: null, created_at: '2024-01-01T00:00:00Z', updated_at: '2024-01-01T00:00:00Z' });
  }

  deleteCollectionItem(): Observable<void> {
    return of(void 0);
  }
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
    stub.metadata = {
      '1': {
        status: 'published',
        published_at: '2024-01-01T00:00:00Z',
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-01-02T00:00:00Z',
      },
    };
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
    stub.metadata = {
      '1': {
        status: 'draft',
        published_at: null,
        created_at: '2024-01-01T00:00:00Z',
        updated_at: '2024-05-06T07:08:09Z',
      },
    };
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
});
