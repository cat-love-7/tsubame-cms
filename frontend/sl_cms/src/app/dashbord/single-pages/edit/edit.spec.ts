import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';
import { Observable, of } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { ItemMetadata } from 'app/models/item-status';
import { SinglePagesService } from 'app/services/schema/single_pages.service';

import { t } from 'app/core/i18n/message';

import { Edit } from './edit';

class StubSinglePagesService {
  /** The pages the screen asked about, so a switch can be told from a first load. */
  public requested: string[] = [];
  /** What the page currently holds, for a test that switches pages. */
  public content: unknown = { title: 'Home' };
  public published: string[] = [];
  public unpublished: string[] = [];
  public metadata: ItemMetadata = {
    status: 'draft',
    published_at: null,
    last_published_at: null,
    published_by: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    has_draft: false,
  };

  /** Saves, and metadata reads, in order. */
  public saved: unknown[] = [];
  public metadataReads = 0;

  getPageSchema(name: string): Observable<unknown> {
    this.requested.push(name);
    return of([{ name: 'title', field_type: 'Text', required: false, width: 12, height: 1 }]);
  }

  getPageItem(): Observable<unknown> {
    return of(this.content);
  }

  getPageMetadata(): Observable<ItemMetadata> {
    this.metadataReads += 1;
    return of(this.metadata);
  }

  updatePageItem(_name: string, values: unknown): Observable<void> {
    this.saved.push(values);
    // The server records the first save as a draft, which is what the badge then shows.
    this.metadata = { ...this.metadata, status: 'draft' };
    return of(void 0);
  }

  publishPage(name: string): Observable<ItemMetadata> {
    this.published.push(name);
    return of({ status: 'published', published_at: '2024-01-01T00:00:00Z',
      last_published_at: '2024-01-01T00:00:00Z', published_by: null, created_at: '2024-01-01T00:00:00Z', updated_at: '2024-01-01T00:00:00Z', has_draft: false });
  }

  unpublishPage(name: string): Observable<ItemMetadata> {
    this.unpublished.push(name);
    return of({ status: 'draft', published_at: null,
    last_published_at: null, published_by: null, created_at: '2024-01-01T00:00:00Z', updated_at: '2024-01-01T00:00:00Z', has_draft: false });
  }
}

/** Whether the screen offers a button with this label (the label is what a user reads). */
function hasButton(element: HTMLElement, label: string): boolean {
  return Array.from(element.querySelectorAll('button')).some((button) =>
    button.textContent?.includes(label),
  );
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
  let route: ReturnType<typeof stubActivatedRoute>;

  beforeEach(async () => {
    stub = new StubSinglePagesService();
    route = stubActivatedRoute({ name: 'home' });
    await TestBed.configureTestingModule({
      imports: [Edit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
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

  // The sidebar switches pages without leaving this route, and the router reuses the component:
  // reading the parameter once left the previous page on screen while the URL changed.
  it('loads another page when the parameter changes', async () => {
    expect(component.pageName()).toBe('home');
    stub.content = { title: 'About us' };

    route.navigate({ name: 'about' });
    await fixture.whenStable();
    fixture.detectChanges();

    expect(component.pageName()).toBe('about');
    expect(stub.requested).toContain('about');
    expect(component.values()['title']).toBe('About us');
    expect(fixture.nativeElement.querySelector('h2')?.textContent).toContain('about');
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

  // Saving used to navigate to the schema list, which has no publish control: the reader had to
  // find the page again to put it on the site.
  it('stays on the page after saving, and shows what was saved', () => {
    const fresh = TestBed.createComponent(Edit);
    fresh.componentInstance.values.set({ title: 'About us' });
    fresh.detectChanges();

    fresh.componentInstance.save();
    fresh.detectChanges();

    expect(stub.saved).toEqual([{ title: 'About us' }]);
    expect(fresh.componentInstance.notice()).toEqual(t('common.saved'));
    // The status only exists once the page has been saved, so it is read again.
    expect(stub.metadataReads).toBeGreaterThan(1);
    expect(fresh.nativeElement.querySelector('app-item-status')).toBeTruthy();
  });

  it('saves and publishes in one act', () => {
    const fresh = TestBed.createComponent(Edit);
    fresh.componentInstance.values.set({ title: 'About us' });
    fresh.detectChanges();

    fresh.componentInstance.saveAndPublish();
    fresh.detectChanges();

    expect(stub.saved).toEqual([{ title: 'About us' }]);
    expect(stub.published).toEqual(['home']);
    expect(fresh.componentInstance.published()).toBe(true);
  });

  it('offers no save-and-publish to an account that may not release content', () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      imports: [Edit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: stubActivatedRoute({ name: 'home' }) },
        { provide: SinglePagesService, useValue: stub },
        stubAuth(true, false, false),
      ],
    });
    const fresh = TestBed.createComponent(Edit);
    fresh.detectChanges();

    expect(hasButton(fresh.nativeElement, 'Save')).toBe(true);
    expect(hasButton(fresh.nativeElement, 'Save and publish')).toBe(false);
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
      last_published_at: '2024-01-01T00:00:00Z',
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

  it('offers to release the changes waiting on a published page', () => {
    stub.metadata = {
      status: 'published',
      published_at: '2024-01-01T00:00:00Z',
      last_published_at: '2024-01-01T00:00:00Z',
      published_by: { id: 'u1', username: 'admin@example.com' },
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-02T00:00:00Z',
      has_draft: true,
    };
    const fresh = TestBed.createComponent(Edit);
    fresh.detectChanges();

    expect(publishButton(fresh.nativeElement, 'Publish changes')).toBeTruthy();
    publishButton(fresh.nativeElement, 'Publish changes').click();
    fresh.detectChanges();

    expect(stub.published).toEqual(['home']);
    expect(stub.unpublished).toEqual([]);
    expect(fresh.componentInstance.published()).toBe(true);
    expect(hasButton(fresh.nativeElement, 'Publish changes')).toBe(false);
  });
});
