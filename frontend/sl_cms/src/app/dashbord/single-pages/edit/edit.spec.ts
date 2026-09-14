import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { Observable, of } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { ItemMetadata } from 'app/models/item-status';
import { SinglePagesService } from 'app/services/schema/single_pages.service';

import { t } from 'app/core/i18n/message';

import { Edit } from './edit';

class StubSinglePagesService {
  public published: string[] = [];
  public unpublished: string[] = [];
  public metadata: ItemMetadata = {
    status: 'draft',
    published_at: null,
    published_by: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    has_draft: false,
  };

  getPageSchema(): Observable<unknown> {
    return of([{ name: 'title', field_type: 'Text', required: false, width: 12, height: 1 }]);
  }

  getPageItem(): Observable<unknown> {
    return of({ title: 'Home' });
  }

  getPageMetadata(): Observable<ItemMetadata> {
    return of(this.metadata);
  }

  publishPage(name: string): Observable<ItemMetadata> {
    this.published.push(name);
    return of({ status: 'published', published_at: '2024-01-01T00:00:00Z', published_by: null, created_at: '2024-01-01T00:00:00Z', updated_at: '2024-01-01T00:00:00Z', has_draft: false });
  }

  unpublishPage(name: string): Observable<ItemMetadata> {
    this.unpublished.push(name);
    return of({ status: 'draft', published_at: null, published_by: null, created_at: '2024-01-01T00:00:00Z', updated_at: '2024-01-01T00:00:00Z', has_draft: false });
  }
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
  let component: Edit;
  let fixture: ComponentFixture<Edit>;
  let stub: StubSinglePagesService;

  beforeEach(async () => {
    stub = new StubSinglePagesService();
    await TestBed.configureTestingModule({
      imports: [Edit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: { snapshot: { params: { name: 'home' } } } },
        { provide: SinglePagesService, useValue: stub },
        stubAuth(),
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(Edit);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  it('lays fields out on the shared grid', () => {
    const fresh = TestBed.createComponent(Edit);
    fresh.componentInstance.schema.set([
      { name: 'title', field_type: 'Number', required: false, width: 8, height: 1 },
    ]);
    fresh.detectChanges();

    const cell = fresh.nativeElement.querySelector('.field-cell') as HTMLElement;
    expect(cell.style.gridColumn).toBe('span 8');
  });

  it('refuses to save while a field reports a problem', () => {
    const fresh = TestBed.createComponent(Edit);
    const component = fresh.componentInstance;
    component.setFieldError(
      { name: 'body', field_type: 'Number', required: false, width: 12, height: 1 },
      t('content.invalidJson', { field: 'body' }),
    );

    component.save();

    // The message is held as a key, so the wording is the catalog's business.
    expect(component.error()).toEqual({ key: 'content.invalidJson', params: { field: 'body' } });
  });

  it('publishes the page without saving the form', () => {
    fixture.detectChanges();

    const badge = fixture.nativeElement.querySelector('app-item-status .badge') as HTMLElement;
    expect(badge.textContent?.trim()).toBe('Draft');

    publishButton(fixture.nativeElement, 'Publish').click();
    fixture.detectChanges();

    expect(stub.published).toEqual(['home']);
    expect(component.published()).toBe(true);
    expect(publishButton(fixture.nativeElement, 'Unpublish')).toBeTruthy();
  });

  it('unpublishes a page that is currently published', () => {
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

    expect(stub.unpublished).toEqual(['home']);
    expect(fresh.componentInstance.published()).toBe(false);
  });
});
