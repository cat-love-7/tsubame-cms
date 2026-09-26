import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';
import { Observable, Subject, of, throwError } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { ItemMetadata } from 'app/models/item-status';
import { SinglePagesService } from 'app/services/schema/single-pages.service';

import { t } from 'app/core/i18n/message';

import { TypedFixture } from 'app/core/testing/fixture';
import { SinglePageEdit } from './edit';

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

  /** When set, the minted link waits for the test to complete it. */
  public heldPreviews?: Subject<{ path: string; expires_at: string }>;

  createPreviewLink(_name: string): Observable<{ path: string; expires_at: string }> {
    return (
      this.heldPreviews ??
      of({
        path: '/preview/single-pages/home?token=1758000000.abc123',
        expires_at: '2026-09-13T12:00:00Z',
      })
    );
  }

  getPageSchema(name: string): Observable<unknown> {
    this.requested.push(name);
    return of([{ name: 'title', field_type: 'Text', required: false, width: 12, height: 1 }]);
  }

  /** What the schema is told about itself. On by default, so a preview is there to be minted. */
  public settings: { preview: boolean } = { preview: true };

  getPageSettings(): Observable<{ preview: boolean }> {
    return of(this.settings);
  }

  /** When set, the page's content waits for the test to complete it. */
  public heldItem?: Subject<unknown>;

  getPageItem(): Observable<unknown> {
    return this.heldItem ?? of(this.content);
  }

  getPageMetadata(): Observable<ItemMetadata> {
    this.metadataReads += 1;
    return of(this.metadata);
  }

  /** What the site is serving, for the comparison. */
  public publishedItem: unknown = { title: 'Live' };
  /** Every page whose draft this screen asked to discard. */
  public discarded: string[] = [];

  getPublishedPageItem(): Observable<unknown> {
    return of(this.publishedItem);
  }

  discardPageDraft(name: string): Observable<void> {
    this.discarded.push(name);
    return of(void 0);
  }

  /** When set, a save waits for the test to complete it (see the delayed-answer tests). */
  public heldSave?: Subject<void>;
  /** Set to refuse the next save. */
  public saveRefusal: unknown = null;

  updatePageItem(_name: string, values: unknown): Observable<void> {
    this.saved.push(values);
    // The server records the first save as a draft, which is what the badge then shows.
    this.metadata = { ...this.metadata, status: 'draft' };
    if (this.saveRefusal) {
      return throwError(() => this.saveRefusal);
    }
    return this.heldSave ?? of(void 0);
  }

  publishPage(name: string): Observable<ItemMetadata> {
    this.published.push(name);
    return of({
      status: 'published',
      published_at: '2024-01-01T00:00:00Z',
      last_published_at: '2024-01-01T00:00:00Z',
      published_by: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      has_draft: false,
    });
  }

  unpublishPage(name: string): Observable<ItemMetadata> {
    this.unpublished.push(name);
    return of({
      status: 'draft',
      published_at: null,
      last_published_at: null,
      published_by: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      has_draft: false,
    });
  }
}

/** The form's save button, by its exact label ("Save and publish" is a different button). */
function saveButton(element: HTMLElement): HTMLButtonElement {
  const button = Array.from(element.querySelectorAll('button')).find(
    (candidate) => candidate.textContent?.trim() === 'Save',
  );
  if (!button) {
    throw new Error('no Save button');
  }
  return button;
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
  return button;
}

/** The deployment's answer about the preview site, which is the deployment's business. */
class StubCapabilities {
  /** Set to null to be a deployment that has no preview site. */
  public site: string | null = 'https://preview.example.test';

  previewSiteUrl(): string | null {
    return this.site;
  }

  load(): void {
    // The screen never loads it; the app does.
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
      // The screens ask about the resource they are showing; these stubs answer the same way
      // everywhere.
      canEditIn: () => canEdit,
      canPublishIn: () => canPublish,
      isAdmin: () => isAdmin,
    },
  };
}

describe('SinglePageEdit', () => {
  let component: SinglePageEdit;
  let fixture: TypedFixture<SinglePageEdit>;
  let stub: StubSinglePagesService;
  let capabilities: StubCapabilities;
  let route: ReturnType<typeof stubActivatedRoute>;

  beforeEach(async () => {
    stub = new StubSinglePagesService();
    capabilities = new StubCapabilities();
    route = stubActivatedRoute({ name: 'home' });
    await TestBed.configureTestingModule({
      imports: [SinglePageEdit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
        { provide: SinglePagesService, useValue: stub },
        { provide: CapabilitiesService, useValue: capabilities },
        stubAuth(),
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(SinglePageEdit);
    component = fixture.componentInstance;
    await fixture.whenStable();
  });

  // See the collection item editor: the page's content has to arrive before the form can be
  // edited or saved, or a save would write the empty form over it.
  it('does not offer to edit or save before the page has loaded', async () => {
    const slow = new Subject<unknown>();
    stub.heldItem = slow;
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    fresh.detectChanges();
    const component = fresh.componentInstance;

    expect(component.loaded()).toBe(false);
    expect(saveButton(fresh.nativeElement).disabled).toBe(true);
    // Nothing to type into before the content is there (see the collection item editor).
    expect(fresh.nativeElement.querySelector('app-value-field')).toBeNull();

    slow.next({ title: 'Home' });
    slow.complete();
    await fresh.whenStable();
    fresh.detectChanges();

    expect(component.loaded()).toBe(true);
    expect(saveButton(fresh.nativeElement).disabled).toBe(false);
    expect(fresh.nativeElement.querySelector('app-value-field')).toBeTruthy();
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
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    fresh.componentInstance.schema.set([
      { name: 'title', field_type: 'Number', required: false, width: 8, height: 1 },
    ]);
    fresh.detectChanges();

    const cell = fresh.nativeElement.querySelector('.field-cell') as HTMLElement;
    expect(cell.style.gridColumn).toBe('span 8');
  });

  it('refuses to save while a field reports a problem', () => {
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
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
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
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

  // The form's own save button is at the end of a long form, so "save" is offered in the toolbar
  // too - beside the publish controls it belongs with.
  it('offers save in the toolbar as well as at the end of the form', () => {
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    fresh.componentInstance.values.set({ title: 'About us' });
    fresh.detectChanges();

    const labels = Array.from(fresh.nativeElement.querySelectorAll('button'))
      .map((button) => button.textContent?.trim())
      .filter((label) => label === 'Save');
    expect(labels.length).toBe(2);

    // The first one in the DOM is the toolbar's, and it is the same act.
    saveButton(fresh.nativeElement).click();
    fresh.detectChanges();

    expect(stub.saved).toEqual([{ title: 'About us' }]);
  });

  it('saves and publishes in one act', () => {
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    fresh.componentInstance.values.set({ title: 'About us' });
    fresh.detectChanges();

    fresh.componentInstance.saveAndPublish();
    fresh.detectChanges();

    expect(stub.saved).toEqual([{ title: 'About us' }]);
    expect(stub.published).toEqual(['home']);
    expect(fresh.componentInstance.published()).toBe(true);
  });

  // The reader can move to another page while the save is in flight, and the sidebar does it
  // without leaving the route: "publish the current page" then means the one they moved *to*,
  // which is the page that goes live. The act carries the page it was pressed for instead.
  it('publishes the page the save was about, not the one on screen when it lands', async () => {
    const held = new Subject<void>();
    stub.heldSave = held;
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    const component = fresh.componentInstance;
    component.values.set({ title: 'About us' });
    fresh.detectChanges();

    component.saveAndPublish();
    // The sidebar switches pages before the answer arrives.
    route.navigate({ name: 'contact' });
    await fresh.whenStable();
    held.next();
    held.complete();
    await fresh.whenStable();

    expect(stub.saved).toEqual([{ title: 'About us' }]);
    expect(stub.published).toEqual([]);
  });

  // The same delay, the other way round: the answer settles the *form*, and a form that no longer
  // belongs to that page must not be told it has been saved - or that the save failed.
  it('writes nothing into the page the reader moved to', async () => {
    const held = new Subject<void>();
    stub.heldSave = held;
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    const component = fresh.componentInstance;
    component.values.set({ title: 'About us' });
    fresh.detectChanges();

    component.save();
    route.navigate({ name: 'contact' });
    await fresh.whenStable();
    held.next();
    held.complete();
    await fresh.whenStable();
    fresh.detectChanges();

    expect(component.notice()).toBeNull();
    expect(component.error()).toBeNull();
    expect(component.hasUnsavedChanges()).toBe(false);
    expect(component.values()).toEqual({ title: 'Home' });
  });

  // The same delay one step further: leaving the route destroys the screen, and a "save and
  // publish" that answers afterwards must not release the page from wherever the reader has gone.
  it('publishes nothing when the save answers after the screen was destroyed', async () => {
    const held = new Subject<void>();
    stub.heldSave = held;
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    const component = fresh.componentInstance;
    component.values.set({ title: 'About us' });
    fresh.detectChanges();

    component.saveAndPublish();
    fresh.destroy();
    held.next();
    held.complete();
    await fresh.whenStable();

    // The save had already been sent; the answer must not turn it into a publish.
    expect(stub.saved).toEqual([{ title: 'About us' }]);
    expect(stub.published).toEqual([]);
    expect(component.notice()).toBeNull();
  });

  it('reports nothing about a save that failed on the page the reader left', async () => {
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    const component = fresh.componentInstance;
    component.values.set({ title: 'About us' });
    fresh.detectChanges();
    stub.saveRefusal = { error: { code: 'field_required', field: 'title', message: 'required' } };
    stub.heldSave = undefined;

    // The save is asked for on `home`, and refused after the reader switched to `contact`.
    const refusal = throwError(() => stub.saveRefusal);
    vi.spyOn(stub, 'updatePageItem').mockReturnValue(
      new Observable((subscriber) => {
        route.navigate({ name: 'contact' });
        refusal.subscribe(subscriber);
      }),
    );
    component.save();
    await fresh.whenStable();
    fresh.detectChanges();

    expect(component.error()).toBeNull();
    expect(component.problemField()).toBeNull();
  });

  // The same comparison and undo as the collection item editor, against this page's own routes.
  it('compares the saved changes with what is published, and can discard them', async () => {
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    stub.metadata = {
      ...stub.metadata,
      status: 'published',
      published_at: '2024-01-01T00:00:00Z',
      last_published_at: '2024-01-01T00:00:00Z',
      has_draft: true,
    };
    stub.content = { title: 'Changed' };
    stub.publishedItem = { title: 'Live' };
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    fresh.detectChanges();
    await fresh.whenStable();
    const component = fresh.componentInstance;

    const button = (label: string): HTMLButtonElement | undefined =>
      Array.from(fresh.nativeElement.querySelectorAll('button')).find((candidate) =>
        candidate.textContent?.includes(label),
      );
    button('Compare with what is published')?.click();
    fresh.detectChanges();
    await fresh.whenStable();

    expect(component.changedFields().map((change) => change.field.name)).toEqual(['title']);
    const sides = Array.from(
      fresh.nativeElement.querySelectorAll<HTMLInputElement>('.comparison input'),
      (input) => input.value,
    );
    expect(sides).toEqual(['Live', 'Changed']);

    button('Discard the changes')?.click();
    fresh.detectChanges();
    await fresh.whenStable();

    expect(confirmSpy).toHaveBeenCalled();
    expect(stub.discarded).toEqual(['home']);
    expect(component.notice()).toEqual({ key: 'content.changesDiscarded' });
  });

  // A schema with previews turned off offers no button at all (see the collection item editor).
  it('does not offer a preview link when the schema does not allow one', async () => {
    stub.settings = { preview: false };
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
    fresh.detectChanges();
    await fresh.whenStable();

    const labels = Array.from(fresh.nativeElement.querySelectorAll('button')).map(
      (button) => button.textContent ?? '',
    );
    expect(labels.some((label) => label.includes('Preview link'))).toBe(false);
  });

  // A deployment with no preview site has nothing readable to hand over (see the collection item
  // editor): the API's own answer is JSON.
  it('says so when the deployment has no preview site', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    try {
      capabilities.site = null;
      component.sharePreview();
      await fixture.whenStable();
      fixture.detectChanges();

      expect(component.error()).toEqual(t('content.previewSiteNotConfigured'));
      expect(component.previewUrl()).toBe('');
      expect(component.notice()).toBeNull();
      expect(writeText).not.toHaveBeenCalled();
    } finally {
      delete (navigator as unknown as Record<string, unknown>)['clipboard'];
    }
  });

  // A link is minted for the page on screen when the button is pressed, and the sidebar switches
  // pages without leaving the route: a link for the page the reader has left is neither theirs to
  // show nor theirs to copy.
  it('ignores a preview link for the page the reader left', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    try {
      const held = new Subject<{ path: string; expires_at: string }>();
      stub.heldPreviews = held;
      const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
      const component = fresh.componentInstance;
      fresh.detectChanges();

      component.sharePreview();
      route.navigate({ name: 'contact' });
      await fresh.whenStable();
      held.next({
        path: '/preview/single-pages/home?token=1758000000.abc123',
        expires_at: '2026-09-13T12:00:00Z',
      });
      held.complete();
      await fresh.whenStable();
      fresh.detectChanges();

      expect(component.previewUrl()).toBe('');
      expect(component.notice()).toBeNull();
      expect(writeText).not.toHaveBeenCalled();
    } finally {
      // The other tests in this file rely on there being no clipboard in this environment.
      delete (navigator as unknown as Record<string, unknown>)['clipboard'];
    }
  });

  it('offers no save-and-publish to an account that may not release content', () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      imports: [SinglePageEdit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: stubActivatedRoute({ name: 'home' }) },
        { provide: SinglePagesService, useValue: stub },
        stubAuth(true, false, false),
      ],
    });
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
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
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
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
    const fresh: TypedFixture<SinglePageEdit> = TestBed.createComponent(SinglePageEdit);
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
