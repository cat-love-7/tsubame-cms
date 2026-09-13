import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivatedRoute } from '@angular/router';
import { provideRouter } from '@angular/router';
import { Observable, of } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { ItemMetadata } from 'app/models/item-status';
import { CollectionsService } from 'app/services/schema/collections.service';

import { Edit } from './edit';

class StubCollectionsService {
  public published: number[] = [];
  public unpublished: number[] = [];
  public metadata: ItemMetadata = {
    status: 'draft',
    published_at: null,
    published_by: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    has_draft: false,
  };

  getCollectionSchema(): Observable<unknown> {
    return of([{ name: 'title', field_type: 'Text', required: false, width: 12, height: 1 }]);
  }

  getCollectionItem(): Observable<unknown> {
    return of({ title: 'Hello' });
  }

  getItemMetadata(): Observable<ItemMetadata> {
    return of(this.metadata);
  }

  publishItem(_name: string, id: number): Observable<ItemMetadata> {
    this.published.push(id);
    return of({ status: 'published', published_at: '2024-01-01T00:00:00Z', published_by: null, created_at: '2024-01-01T00:00:00Z', updated_at: '2024-01-01T00:00:00Z', has_draft: false });
  }

  createPreviewLink(_name: string, _id: number): Observable<{ path: string; expires_at: string }> {
    return of({
      path: '/preview/collections/blog/items/7?token=1758000000.abc123',
      expires_at: '2026-09-13T12:00:00Z',
    });
  }

  unpublishItem(_name: string, id: number): Observable<ItemMetadata> {
    this.unpublished.push(id);
    return of({ status: 'draft', published_at: null, published_by: null, created_at: '2024-01-01T00:00:00Z', updated_at: '2024-01-01T00:00:00Z', has_draft: false });
  }
}

function previewButton(element: HTMLElement): HTMLButtonElement {
  return Array.from(element.querySelectorAll('button')).find((button) =>
    button.textContent?.includes('プレビュー URL'),
  ) as HTMLButtonElement;
}

function publishButton(element: HTMLElement, label: string): HTMLButtonElement {
  const button = Array.from(element.querySelectorAll('button')).find((candidate) =>
    candidate.textContent?.includes(label),
  );
  if (!button) {
    throw new Error(`no button labelled "${label}"`);
  }
  return button as HTMLButtonElement;
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

describe('Edit', () => {
  let stub: StubCollectionsService;

  beforeEach(async () => {
    stub = new StubCollectionsService();
    await TestBed.configureTestingModule({
      imports: [Edit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        // Editing item 7 of the `blog` collection.
        { provide: ActivatedRoute, useValue: { snapshot: { params: { name: 'blog', id: '7' } } } },
        { provide: CollectionsService, useValue: stub },
        stubAuth(),
      ],
    }).compileComponents();
  });

  it('should create', () => {
    const fixture = TestBed.createComponent(Edit);
    const component = fixture.componentInstance;
    fixture.detectChanges();

    expect(component).toBeTruthy();
  });

  // The schema's width/height used to be ignored here entirely, which made the layout
  // shown by the schema editor meaningless.
  it('lays fields out using the width and height from the schema', () => {
    const fresh = TestBed.createComponent(Edit);
    fresh.componentInstance.schema.set([
      { name: 'a', field_type: 'Number', required: false, width: 6, height: 2 },
      { name: 'b', field_type: 'Boolean', required: false, width: 6, height: 1 },
    ]);
    fresh.detectChanges();

    const cells = fresh.nativeElement.querySelectorAll('.field-cell') as NodeListOf<HTMLElement>;
    expect(cells.length).toBe(2);
    expect(cells[0].style.gridColumn).toBe('span 6');
    expect(cells[0].style.minHeight).toBe('calc(var(--field-row-unit, 72px) * 2)');
    expect(cells[1].style.minHeight).toBe('calc(var(--field-row-unit, 72px) * 1)');
  });

  // Bad input used to be caught by normalising the JSON buffers at save time; now the
  // value fields own their input and report problems, so the refusal lives here.
  it('refuses to save while a field reports a problem', () => {
    const fresh = TestBed.createComponent(Edit);
    const component = fresh.componentInstance;
    component.setFieldError(
      { name: 'numbers', field_type: 'Number', required: false, width: 12, height: 1 },
      "Field 'numbers': invalid JSON",
    );

    component.save();

    expect(component.error()).toContain('invalid JSON');
  });

  it('says that a saved change is not published yet', () => {
    // Only a published item with pending changes says so; a draft is obvious already.
    stub.metadata = { ...stub.metadata, status: 'published', has_draft: true };
    const fresh = TestBed.createComponent(Edit);
    fresh.detectChanges();

    expect(fresh.nativeElement.querySelector('.draft-note')).toBeTruthy();
    expect(fresh.nativeElement.textContent).toContain('まだ公開されていません');
  });

  /** Asking for a shareable link shows it, so it can be copied even if the clipboard says no. */
  it('mints a preview link and shows it with its expiry', async () => {
    const fresh = TestBed.createComponent(Edit);
    fresh.detectChanges();

    const button = previewButton(fresh.nativeElement);
    expect(button).toBeTruthy();
    button.click();
    await fresh.whenStable();
    fresh.detectChanges();

    const component = fresh.componentInstance;
    expect(component.previewUrl()).toBe(
      `${location.origin}/api/preview/collections/blog/items/7?token=1758000000.abc123`,
    );
    // The link is rendered as a real anchor as well as offered to the clipboard.
    const anchor = fresh.nativeElement.querySelector('.preview-link a') as HTMLAnchorElement;
    expect(anchor.getAttribute('href')).toBe(component.previewUrl());
    expect(component.notice()).toContain('有効期限');
  });

  it('publishes the item it is editing without saving the form', () => {
    const fresh = TestBed.createComponent(Edit);
    fresh.detectChanges();

    const badge = fresh.nativeElement.querySelector('app-item-status .badge') as HTMLElement;
    expect(badge.textContent?.trim()).toBe('Draft');

    publishButton(fresh.nativeElement, 'Publish').click();
    fresh.detectChanges();

    expect(stub.published).toEqual([7]);
    expect(fresh.componentInstance.published()).toBe(true);
    expect(fresh.nativeElement.querySelector('app-item-status .badge').textContent?.trim()).toBe(
      'Published',
    );
    // The control now offers the opposite action.
    expect(publishButton(fresh.nativeElement, 'Unpublish')).toBeTruthy();
  });

  it('unpublishes an item that is currently published', () => {
    stub.metadata = {
      status: 'published',
      published_at: '2024-01-01T00:00:00Z',
      published_by: { id: 'u1', username: 'admin@example.com' },
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    has_draft: false,
    };
    const fresh = TestBed.createComponent(Edit);
    fresh.detectChanges();

    // The editor names whoever published the version that is live.
    const publisher = fresh.nativeElement.querySelector('.publisher') as HTMLElement;
    expect(publisher.textContent?.trim()).toBe('admin@example.com');

    publishButton(fresh.nativeElement, 'Unpublish').click();
    fresh.detectChanges();

    expect(stub.unpublished).toEqual([7]);
    expect(fresh.componentInstance.published()).toBe(false);
  });
});

describe('Edit (new item)', () => {
  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [Edit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: { snapshot: { params: { name: 'blog' } } } },
        { provide: CollectionsService, useValue: new StubCollectionsService() },
      ],
    }).compileComponents();
  });

  /** There is no status to show until the item exists on the server. */
  it('offers no publish control before the item is saved', () => {
    const fresh = TestBed.createComponent(Edit);
    fresh.detectChanges();

    expect(fresh.componentInstance.isNew).toBe(true);
    expect(fresh.nativeElement.querySelector('app-item-status')).toBeNull();
  });
});
