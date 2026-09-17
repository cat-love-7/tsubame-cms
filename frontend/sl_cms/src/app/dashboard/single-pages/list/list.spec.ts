import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { Observable, of, throwError } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { ItemMetadata } from 'app/models/item-status';
import { SinglePagesService } from 'app/services/schema/single-pages.service';

import { SinglePageList } from './list';

function metadata(overrides: Partial<ItemMetadata> = {}): ItemMetadata {
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

/** A fake pair of pages: one live, one still being written. */
class StubSinglePagesService {
  public names: string[] = ['about', 'contact'];
  public statuses: { [name: string]: ItemMetadata } = {
    about: metadata({
      status: 'published',
      published_at: '2024-02-01T00:00:00Z',
      last_published_at: '2024-02-01T00:00:00Z',
      published_by: { id: '1', username: 'admin@example.com' },
    }),
    contact: metadata(),
  };
  public published: string[] = [];
  public unpublished: string[] = [];
  /** Set to refuse a publish the way the server refuses one. */
  public refusal: unknown = null;

  listPageNames = () => of(this.names);
  listItemMetadata = () => of(this.statuses);

  publishPage(name: string): Observable<ItemMetadata> {
    if (this.refusal) {
      return throwError(() => this.refusal);
    }
    this.published.push(name);
    const published = metadata({
      status: 'published',
      published_at: '2024-02-01T00:00:00Z',
      last_published_at: '2024-02-01T00:00:00Z',
    });
    this.statuses = { ...this.statuses, [name]: published };
    return of(published);
  }

  unpublishPage(name: string): Observable<ItemMetadata> {
    this.unpublished.push(name);
    const draft = metadata();
    this.statuses = { ...this.statuses, [name]: draft };
    return of(draft);
  }
}

/** Permissions are the server's business; the screens are only told what to offer. */
function stubAuth(canEdit = true, canPublish = true, isAdmin = true) {
  return {
    provide: AuthService,
    useValue: {
      user: () => null,
      canEdit: () => canEdit,
      canPublish: () => canPublish,
      canEditIn: () => canEdit,
      canPublishIn: () => canPublish,
      isAdmin: () => isAdmin,
    },
  };
}

describe('Single page list', () => {
  let component: SinglePageList;
  let fixture: ComponentFixture<SinglePageList>;
  let stub: StubSinglePagesService;

  beforeEach(async () => {
    stub = new StubSinglePagesService();
    await TestBed.configureTestingModule({
      imports: [SinglePageList],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: SinglePagesService, useValue: stub },
        stubAuth(),
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(SinglePageList);
    component = fixture.componentInstance;
    await fixture.whenStable();
    fixture.detectChanges();
  });

  function rows(): HTMLElement[] {
    return Array.from(fixture.nativeElement.querySelectorAll('table.items tbody tr'));
  }

  function rowFor(name: string): HTMLElement {
    const row = rows().find((candidate) => candidate.textContent?.includes(name));
    if (!row) {
      throw new Error(`no row for ${name}`);
    }
    return row;
  }

  function button(element: HTMLElement, label: string): HTMLButtonElement | undefined {
    return Array.from(element.querySelectorAll('button')).find((candidate) =>
      (candidate.getAttribute('aria-label') ?? '').includes(label),
    ) as HTMLButtonElement | undefined;
  }

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  // A single page is one item, so the list is an overview: what is live, what is waiting, and
  // when it last changed.
  it('shows every page with its state', () => {
    expect(rows().length).toBe(2);
    expect(rowFor('about').textContent).toContain('Published');
    expect(rowFor('about').textContent).toContain('admin@example.com');
    expect(rowFor('contact').textContent).toContain('Draft');
  });

  it('marks a published page that has unpublished changes', async () => {
    stub.statuses = {
      ...stub.statuses,
      about: { ...stub.statuses['about'], has_draft: true },
    };
    const fresh = TestBed.createComponent(SinglePageList);
    await fresh.whenStable();
    fresh.detectChanges();

    expect(fresh.nativeElement.querySelectorAll('.draft-note').length).toBe(1);
  });

  it('publishes and unpublishes from the list, without opening the page', () => {
    button(rowFor('contact'), 'publish page')?.click();
    fixture.detectChanges();

    expect(stub.published).toEqual(['contact']);
    expect(rowFor('contact').textContent).toContain('Published');

    button(rowFor('about'), 'unpublish page')?.click();
    fixture.detectChanges();

    expect(stub.unpublished).toEqual(['about']);
    expect(rowFor('about').textContent).toContain('Draft');
  });

  it('reports a refusal instead of pretending it worked', () => {
    stub.refusal = new Error('nope');
    button(rowFor('contact'), 'publish page')?.click();
    fixture.detectChanges();

    expect(component.error()).toEqual({
      key: 'content.failedToChangePublished',
      params: { message: 'nope' },
    });
    expect(rowFor('contact').textContent).toContain('Draft');
  });

  it('offers nothing a read-only account could not do', async () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      imports: [SinglePageList],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: SinglePagesService, useValue: stub },
        stubAuth(false, false, false),
      ],
    });
    const fresh = TestBed.createComponent(SinglePageList);
    await fresh.whenStable();
    fresh.detectChanges();

    expect(fresh.nativeElement.querySelectorAll('button[aria-label]').length).toBe(0);
  });

  it('says so when there are no pages yet', async () => {
    stub.names = [];
    stub.statuses = {};
    const fresh = TestBed.createComponent(SinglePageList);
    await fresh.whenStable();
    fresh.detectChanges();

    expect(fresh.nativeElement.querySelector('.empty')?.textContent).toContain('No single pages');
  });
});
