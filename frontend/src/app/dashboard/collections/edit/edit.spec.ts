import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router } from '@angular/router';
import { provideRouter } from '@angular/router';
import { stubActivatedRoute } from 'app/core/testing/activated-route';
import { Observable, Subject, defer, of, throwError } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { ItemMetadata } from 'app/models/item-status';
import { CollectionsService } from 'app/services/schema/collections.service';

import { t } from 'app/core/i18n/message';

import { TypedFixture } from 'app/core/testing/fixture';
import { CollectionItemEdit } from './edit';
import { formatDateTime } from 'app/core/i18n/date-format';

/** A metadata record with the fields a test cares about, and the rest as a draft. */
function metadataWith(overrides: Partial<ItemMetadata>): ItemMetadata {
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

class StubCollectionsService {
  /** The items the screen asked about, so a switch can be told from a first load. */
  public requested: number[] = [];
  public published: number[] = [];
  public unpublished: number[] = [];
  /** Set to refuse a save the way the server refuses a value another item holds. */
  public saveRefusal: unknown = null;
  public metadata: ItemMetadata = {
    status: 'draft',
    published_at: null,
    last_published_at: null,
    published_by: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    has_draft: false,
  };

  /** Saves the screen sent, so the order of "save, then publish" can be read back. */
  public updated: { id: number; values: unknown }[] = [];
  /** Where a publish was sent, which is how "the wrong collection" would show up. */
  public publishedTargets: { name: string; id: number }[] = [];
  /** When set, the save waits for the test to complete it. */
  public heldUpdates?: Subject<void>;

  updateCollectionItem(_name: string, id: number, values: unknown): Observable<void> {
    this.updated.push({ id, values });
    if (this.saveRefusal) {
      return throwError(() => this.saveRefusal);
    }
    return this.heldUpdates ?? of(void 0);
  }

  createCollectionItem(): Observable<number> {
    return this.saveRefusal ? throwError(() => this.saveRefusal) : of(11);
  }

  /** What the collection's schema is, and the item being edited. */
  public schema: unknown[] = [
    { name: 'title', field_type: 'Text', required: false, width: 12, height: 1 },
  ];
  public item: unknown = { title: 'Hello' };

  getCollectionSchema(): Observable<unknown> {
    return of(this.schema);
  }

  /** What the schema is told about itself. On by default, so a preview is there to be minted. */
  public settings: { preview: boolean } = { preview: true };

  getCollectionSettings(): Observable<{ preview: boolean }> {
    return of(this.settings);
  }

  /**
   * Answers the test delivers by hand, by item id.
   *
   * A response that arrives late is the point of one of these tests, and `of(...)` answers before
   * anything can happen in between.
   */
  public heldItems = new Map<number, Subject<unknown>>();

  getCollectionItem(_name: string, id: number): Observable<unknown> {
    this.requested.push(id);
    return this.heldItems.get(id) ?? of(this.item);
  }

  getItemMetadata(): Observable<ItemMetadata> {
    return of(this.metadata);
  }

  /** What the site is serving, for the comparison. `null` means the item is not published. */
  public publishedItem: unknown = { title: 'Hello' };
  /** Every draft this screen asked to discard. */
  public discarded: number[] = [];

  getPublishedItem(_name: string, _id: number): Observable<unknown> {
    return of(this.publishedItem);
  }

  discardItemDraft(_name: string, id: number): Observable<void> {
    this.discarded.push(id);
    return of(void 0);
  }

  publishItem(_name: string, id: number): Observable<ItemMetadata> {
    this.published.push(id);
    this.publishedTargets.push({ name: _name, id });
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

  /** When set, the minted link waits for the test to complete it. */
  public heldPreviews?: Subject<{ path: string; expires_at: string }>;

  createPreviewLink(_name: string, _id: number): Observable<{ path: string; expires_at: string }> {
    return (
      this.heldPreviews ??
      of({
        path: '/preview/collections/blog/items/7?token=1758000000.abc123',
        expires_at: '2026-09-13T12:00:00Z',
      })
    );
  }

  unpublishItem(_name: string, id: number): Observable<ItemMetadata> {
    this.unpublished.push(id);
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

/** The button that shows what is published beside what is saved. */
function compareButton(element: HTMLElement): HTMLButtonElement {
  return buttonWith(element, 'Compare with what is published');
}

/** The button that throws the saved changes away. */
function discardButton(element: HTMLElement): HTMLButtonElement {
  return buttonWith(element, 'Discard the changes');
}

function buttonWith(element: HTMLElement, label: string): HTMLButtonElement {
  return Array.from(element.querySelectorAll('button')).find((button) =>
    button.textContent?.includes(label),
  ) as HTMLButtonElement;
}

function previewButton(element: HTMLElement): HTMLButtonElement {
  return Array.from(element.querySelectorAll('button')).find((button) =>
    button.textContent?.includes('Preview link'),
  ) as HTMLButtonElement;
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

describe('CollectionItemEdit', () => {
  let stub: StubCollectionsService;
  let capabilities: StubCapabilities;
  let route: ReturnType<typeof stubActivatedRoute>;

  beforeEach(async () => {
    stub = new StubCollectionsService();
    capabilities = new StubCapabilities();
    // Editing item 7 of the `blog` collection.
    route = stubActivatedRoute({ name: 'blog', id: '7' });
    await TestBed.configureTestingModule({
      imports: [CollectionItemEdit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: route },
        { provide: CollectionsService, useValue: stub },
        { provide: CapabilitiesService, useValue: capabilities },
        stubAuth(),
      ],
    }).compileComponents();
  });

  // Opening another item reuses this component, so the parameter has to be followed rather than
  // read once: the URL changed while the previous item's values stayed in the form.
  it('loads another item when the parameter changes', async () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    expect(stub.requested).toEqual([7]);

    stub.item = { title: 'Another' };
    route.navigate({ name: 'blog', id: '9' });
    await fixture.whenStable();
    fixture.detectChanges();

    expect(stub.requested).toEqual([7, 9]);
    expect(fixture.componentInstance.values()['title']).toBe('Another');
  });

  // Switching collection keeps the same route, so the schema and the rows have to follow too.
  it('loads another collection when the name changes', async () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();

    route.navigate({ name: 'pages', id: '7' });
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.componentInstance.collectionName()).toBe('pages');
  });

  // An answer that belongs to the item that was open before must not land in the new form: it used
  // to replace it, which showed one item's content under another's address.
  it('ignores an answer for the item that was open before', async () => {
    const slowFirst = new Subject<unknown>();
    stub.heldItems.set(7, slowFirst);
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();

    // Switch to item 9 while item 7's answer is still on its way, and let 9 arrive.
    stub.item = { title: 'Second' };
    route.navigate({ name: 'blog', id: '9' });
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.componentInstance.values()['title']).toBe('Second');

    // Now the first answer turns up. It is about an item nobody is looking at.
    slowFirst.next({ title: 'First' });
    slowFirst.complete();
    await fixture.whenStable();
    fixture.detectChanges();

    expect(fixture.componentInstance.values()['title']).toBe('Second');
    expect(fixture.componentInstance.isNew()).toBe(false);
  });

  // Saving used to land back on the list, so a second edit meant finding the item again. The list
  // is one button away; the save should leave the reader where they are.
  it('stays on the item after a plain save', async () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    const component = fixture.componentInstance;
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigate');

    component.setValue(
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      'Edited',
    );
    component.save();
    await fixture.whenStable();

    expect(stub.updated).toEqual([{ id: 7, values: { title: 'Edited' } }]);
    expect(navigate).not.toHaveBeenCalled();
  });

  // A new item has no address until the create answers, so that much still moves - to the item the
  // save created, not back to the list it came from.
  it("moves a new item's address to the id its save answered", async () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    const component = fixture.componentInstance;
    route.navigate({ name: 'blog' });
    await fixture.whenStable();
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigate');

    component.setValue(
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      'New one',
    );
    component.save();
    await fixture.whenStable();

    expect(navigate).toHaveBeenCalledWith(['/collections', 'blog', 'edit', 11], {
      replaceUrl: true,
    });
  });

  // Leaving is its own act: it goes to the list and it does not save, which "Cancel" said neither
  // of - and a reader who has just saved is not cancelling anything.
  it('names the way out of the screen for what it does', () => {
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    const back = Array.from(fresh.nativeElement.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Back to the list without saving',
    );
    expect(back).toBeTruthy();
  });

  // The toolbar's save sits with the other actions rather than before the state they act on: the
  // status and the publisher it is read against come first.
  it('puts the toolbar save after the state it acts on', () => {
    stub.metadata = {
      ...stub.metadata,
      published_by: { id: 'u1', username: 'admin@example.com' },
    };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    const publisher = fresh.nativeElement.querySelector('.publisher') as HTMLElement;
    const save = saveButton(fresh.nativeElement);
    expect(publisher.compareDocumentPosition(save) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  // "Save" and "Save and publish" belong together: the form's own button is at the end of a long
  // form, so the same act is offered in the toolbar as well.
  it('offers save in the toolbar as well as at the end of the form', async () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();

    const labels = Array.from(fixture.nativeElement.querySelectorAll('button'))
      .map((button) => button.textContent?.trim())
      .filter((label) => label === 'Save');
    expect(labels.length).toBe(2);

    // The first one in the DOM is the toolbar's, and it is the same act.
    saveButton(fixture.nativeElement).click();
    await fixture.whenStable();

    expect(stub.updated).toEqual([{ id: 7, values: { title: 'Hello' } }]);
  });

  // Publishing copies what the server has, so with edits still in the form it would put the
  // previous version on the site while the screen showed the new one.
  it('saves before publishing when the form has unsaved edits, and only then', async () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    const component = fixture.componentInstance;
    component.metadata.set({ ...stub.metadata, status: 'published', has_draft: true });
    fixture.detectChanges();
    expect(component.unsavedChanges()).toBe(false);

    // The button is the plain release while the form is in step with the server.
    expect(hasButton(fixture.nativeElement, 'Publish changes')).toBe(true);
    expect(hasButton(fixture.nativeElement, 'Save and publish')).toBe(false);

    component.setValue(
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      'Edited',
    );
    fixture.detectChanges();
    expect(component.unsavedChanges()).toBe(true);
    expect(hasButton(fixture.nativeElement, 'Save and publish')).toBe(true);

    component.saveAndPublish();
    await fixture.whenStable();

    // Saved, then published: the order is the whole point.
    expect(stub.updated).toEqual([{ id: 7, values: { title: 'Edited' } }]);
    expect(stub.published).toEqual([7]);
  });

  // The reader can switch collections while the save is in flight, and the id alone would then
  // name a *different* item in the collection they moved to. The act carries the collection it was
  // pressed for, and does nothing at all if the reader has moved on.
  it('publishes nothing when the reader left before the save landed', async () => {
    const held = new Subject<void>();
    stub.heldUpdates = held;
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    const component = fixture.componentInstance;
    component.setValue(
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      'Edited',
    );

    component.saveAndPublish();
    // Another collection, before the save answers.
    route.navigate({ name: 'pages', id: '7' });
    await fixture.whenStable();
    held.next();
    held.complete();
    await fixture.whenStable();

    expect(stub.updated).toEqual([{ id: 7, values: { title: 'Edited' } }]);
    expect(stub.publishedTargets).toEqual([]);
    // And the form the reader is looking at was told nothing about the item they left.
    expect(component.notice()).toBeNull();
    expect(component.error()).toBeNull();
    expect(component.hasUnsavedChanges()).toBe(false);
  });

  // The failure side of the same delay: a refusal about the item that was left must not mark the
  // fields of the item now on screen.
  // Leaving the screen destroys the component, and a save already on its way used to pass the
  // guard anyway: the signals it compared were still the ones it captured. The answer then took the
  // reader back to the list the save was pressed from, from wherever they had gone.
  it('does nothing when a save answers after the screen was destroyed', async () => {
    const held = new Subject<void>();
    stub.heldUpdates = held;
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    const component = fixture.componentInstance;
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigate');
    component.setValue(
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      'Edited',
    );

    component.save();
    fixture.destroy();
    held.next();
    held.complete();
    await fixture.whenStable();

    // The save had already been sent, so it reached the server; what must not happen is the screen
    // acting on the answer: no trip back to the list, and nothing written to a form nobody sees.
    expect(stub.updated).toEqual([{ id: 7, values: { title: 'Edited' } }]);
    expect(navigate).not.toHaveBeenCalled();
    expect(component.notice()).toBeNull();
    expect(component.error()).toBeNull();
  });

  it('reports nothing about a save that failed on the item the reader left', async () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    const component = fixture.componentInstance;
    component.setValue(
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      'Edited',
    );
    stub.saveRefusal = { error: { code: 'field_required', field: 'title', message: 'required' } };
    vi.spyOn(stub, 'updateCollectionItem').mockImplementation(
      (_name: string, _id: number) =>
        new Observable((subscriber) => {
          route.navigate({ name: 'pages', id: '7' });
          subscriber.error(stub.saveRefusal);
        }),
    );

    component.saveAndPublish();
    await fixture.whenStable();
    fixture.detectChanges();

    expect(component.error()).toBeNull();
    expect(component.problemField()).toBeNull();
  });

  it('does not publish when the save was refused', async () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    const component = fixture.componentInstance;
    stub.saveRefusal = { error: { code: 'value_taken', field: 'title', message: 'taken' } };
    component.setValue(
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      'Edited',
    );

    component.saveAndPublish();
    await fixture.whenStable();

    expect(stub.published).toEqual([]);
    expect(component.unsavedChanges()).toBe(true);
  });

  it('answers the guard about unsaved edits, and warns a reload', () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    const component = fixture.componentInstance;
    expect(component.hasUnsavedChanges()).toBe(false);

    component.setValue(
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      'Edited',
    );
    expect(component.hasUnsavedChanges()).toBe(true);

    const preventDefault = vi.fn();
    const event = { preventDefault } as unknown as BeforeUnloadEvent;
    component.warnBeforeLeaving(event);
    expect(preventDefault).toHaveBeenCalled();
  });

  // The form holds nothing until the item arrives, and the server holds the item: a save before
  // then would write the empty (or half-typed) form over fields nobody touched.
  it('does not offer to edit or save before the item has loaded', async () => {
    const slow = new Subject<unknown>();
    stub.heldItems.set(7, slow);
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    const component = fresh.componentInstance;

    expect(component.loaded()).toBe(false);
    expect(saveButton(fresh.nativeElement).disabled).toBe(true);
    // Nothing to type into: the form appears with its content, so nothing can be typed into an
    // empty form and then overwritten by the answer.
    expect(fresh.nativeElement.querySelector('app-value-field')).toBeNull();

    // A load that failed keeps the form off the screen rather than offering one that would wipe it.
    slow.error({ error: { code: 'internal', message: 'boom' } });
    await fresh.whenStable();
    fresh.detectChanges();
    expect(component.loaded()).toBe(false);
    expect(saveButton(fresh.nativeElement).disabled).toBe(true);
    expect(fresh.nativeElement.querySelector('app-value-field')).toBeNull();
    expect(fresh.nativeElement.querySelector('.error button')).toBeTruthy();
  });

  it('offers the form once the item has loaded', async () => {
    const slow = new Subject<unknown>();
    stub.heldItems.set(7, slow);
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    const component = fresh.componentInstance;
    expect(saveButton(fresh.nativeElement).disabled).toBe(true);

    slow.next({ title: 'Hello' });
    slow.complete();
    await fresh.whenStable();
    fresh.detectChanges();

    expect(component.loaded()).toBe(true);
    expect(saveButton(fresh.nativeElement).disabled).toBe(false);
    expect(fresh.nativeElement.querySelector('app-value-field')).toBeTruthy();
    // The form is in step with the server, so leaving does not have to be asked about.
    expect(component.unsavedChanges()).toBe(false);
  });

  it('should create', () => {
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    const component = fixture.componentInstance;
    fixture.detectChanges();

    expect(component).toBeTruthy();
  });

  // The schema's width/height used to be ignored here entirely, which made the layout
  // shown by the schema editor meaningless.
  it('lays fields out using the width and height from the schema', () => {
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.componentInstance.schema.set([
      { name: 'a', field_type: 'Number', required: false, width: 6, height: 2 },
      { name: 'b', field_type: 'Boolean', required: false, width: 6, height: 1 },
    ]);
    fresh.detectChanges();

    const cells = fresh.nativeElement.querySelectorAll<HTMLElement>('.field-cell');
    expect(cells.length).toBe(2);
    expect(cells[0].style.gridColumn).toBe('span 6');
    expect(cells[0].style.minHeight).toBe('calc(var(--field-row-unit, 72px) * 2)');
    expect(cells[1].style.minHeight).toBe('calc(var(--field-row-unit, 72px) * 1)');
  });

  // Bad input used to be caught by normalising the JSON buffers at save time; now the
  // value fields own their input and report problems, so the refusal lives here.
  it('refuses to save while a field reports a problem', () => {
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    const component = fresh.componentInstance;
    component.setFieldError(
      { name: 'numbers', field_type: 'Number', required: false, width: 12, height: 1 },
      t('content.invalidJson', { field: 'numbers' }),
    );

    component.save();

    // The message is held as a key, so the wording is the catalog's business.
    expect(component.error()).toEqual({ key: 'content.invalidJson', params: { field: 'numbers' } });
  });

  it('says that a saved change is not published yet', () => {
    // Only a published item with pending changes says so; a draft is obvious already.
    stub.metadata = { ...stub.metadata, status: 'published', has_draft: true };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    expect(fresh.nativeElement.querySelector('.draft-note')).toBeTruthy();
    expect(fresh.nativeElement.textContent).toContain('is not published yet');
  });

  /** Asking for a shareable link shows it, so it can be copied even if the clipboard says no. */
  it('marks the field a refusal is about', () => {
    // The server names the field it refused, so the form can point at that input instead of
    // leaving the reader to work out which one it meant.
    stub.saveRefusal = {
      status: 409,
      error: { code: 'value_taken', message: "field 'title': the value is taken", field: 'title' },
    };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    fresh.componentInstance.save();
    fresh.detectChanges();

    expect(fresh.componentInstance.problemField()).toBe('title');
    expect(fresh.nativeElement.querySelectorAll('.field-cell.problem').length).toBe(1);
    expect((fresh.nativeElement.querySelector('.error') as HTMLElement).textContent).toContain(
      'title',
    );
  });

  // A refusal names the input as a path, so one about something inside a composite still marks
  // the composite's cell rather than nothing at all.
  it('marks the cell a refusal named, including one inside it', () => {
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.componentInstance.schema.set([
      {
        name: 'seo',
        field_type: { CompositeField: { id: 'seo' } },
        required: false,
        width: 12,
        height: 1,
      },
    ]);
    fresh.detectChanges();

    fresh.componentInstance.problemField.set('seo.description');
    fresh.detectChanges();

    expect(fresh.nativeElement.querySelectorAll('.field-cell.problem').length).toBe(1);
  });

  // A problem the widget found itself (a text outside its lengths) is the reader's to fix, so it
  // marks the input exactly like a refusal from the server does.
  it('marks the field a widget reported a problem for, and unmarks it when it is fixed', () => {
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    const component = fresh.componentInstance;
    const title = {
      name: 'title',
      field_type: { Text: { max_length: 5 } },
      required: false,
      width: 12,
      height: 1,
    };

    component.setFieldError(title, t('content.valueTooLong', { field: 'title', max: 5 }));
    expect(component.problemField()).toBe('title');

    component.setFieldError(title, null);
    expect(component.problemField()).toBeNull();
  });

  // A value stored before the limit was lowered is already too long; the form says so at once
  // rather than letting the save fail.
  it('marks a stored value that is longer than the schema now allows', () => {
    stub.schema = [
      {
        name: 'title',
        field_type: { Text: { max_length: 3 } },
        required: false,
        width: 12,
        height: 1,
      },
    ];
    stub.item = { title: 'toolong' };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    expect(fresh.componentInstance.problemField()).toBe('title');
  });

  it('mints a preview link and shows it with its expiry', async () => {
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    const button = previewButton(fresh.nativeElement);
    expect(button).toBeTruthy();
    button.click();
    await fresh.whenStable();
    fresh.detectChanges();

    const component = fresh.componentInstance;
    expect(component.previewUrl()).toBe(
      'https://preview.example.test/preview/collections/blog/items/7?token=1758000000.abc123',
    );
    // The link is rendered as a real anchor as well as offered to the clipboard.
    const anchor = fresh.nativeElement.querySelector('.preview-link a') as HTMLAnchorElement;
    expect(anchor.getAttribute('href')).toBe(component.previewUrl());
    // The expiry is part of the message, in the language on screen. The clipboard is
    // unavailable here, so the message is the one that offers the link to copy by hand.
    expect(component.notice()).toEqual({
      key: 'content.previewNotCopied',
      params: { expires: formatDateTime('2026-09-13T12:00:00Z', 'en') },
    });
  });

  // The form holds the working copy, so without this the content the changes would replace is not
  // visible anywhere in the CMS.
  it('compares the saved changes with what is published, field by field', async () => {
    stub.metadata = metadataWith({ status: 'published', has_draft: true });
    stub.schema = [
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      { name: 'subtitle', field_type: { Text: {} }, required: false, width: 12, height: 1 },
    ];
    stub.item = { title: 'Changed', subtitle: 'Same' };
    stub.publishedItem = { title: 'Hello', subtitle: 'Same' };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    await fresh.whenStable();

    compareButton(fresh.nativeElement).click();
    fresh.detectChanges();
    await fresh.whenStable();

    const component = fresh.componentInstance;
    // Only what would change: a comparison of everything is a second form to read.
    expect(component.changedFields().map((change) => change.field.name)).toEqual(['title']);
    expect(component.changedFields()[0].published).toBe('Hello');
    expect(component.changedFields()[0].draft).toBe('Changed');
    // Rendered by the same read-only value widget the form uses, so the two sides are what an
    // editor would see if either were live.
    const panel = fresh.nativeElement.querySelector('.comparison');
    const sides = Array.from(panel?.querySelectorAll('input') ?? [], (input) => input.value);
    expect(sides).toEqual(['Hello', 'Changed']);
    expect(panel?.textContent).toContain('Published');
    expect(panel?.textContent).toContain('Saved changes');
  });

  // The comparison reads the two versions; it is not a second form. A field whose Schema asks for a
  // tall box used to be drawn at that height on both sides - blank space over one sentence.
  it('shows a tall field in the comparison as text, not at the height the schema asks for', async () => {
    stub.metadata = metadataWith({ status: 'published', has_draft: true });
    stub.schema = [
      { name: 'body', field_type: { Markdown: {} }, required: false, width: 12, height: 12 },
    ];
    stub.item = { body: 'Changed body' };
    stub.publishedItem = { body: 'Old body' };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    await fresh.whenStable();

    compareButton(fresh.nativeElement).click();
    fresh.detectChanges();
    await fresh.whenStable();

    const panel = fresh.nativeElement.querySelector('.comparison');
    expect(panel?.querySelector('textarea')).toBeNull();
    const sides = Array.from(panel?.querySelectorAll('.value-text') ?? [], (side) =>
      side.textContent?.trim(),
    );
    expect(sides).toEqual(['Old body', 'Changed body']);
  });

  it('says so when the saved changes match what is published', async () => {
    stub.metadata = metadataWith({ status: 'published', has_draft: true });
    stub.item = { title: 'Hello' };
    stub.publishedItem = { title: 'Hello' };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    await fresh.whenStable();

    compareButton(fresh.nativeElement).click();
    fresh.detectChanges();
    await fresh.whenStable();

    expect(fresh.componentInstance.changedFields()).toEqual([]);
    expect(fresh.nativeElement.querySelector('.comparison')?.textContent).toContain(
      'the same as what is published',
    );
  });

  // The undo: the site is not touched, and the form goes back to what is being served.
  it('discards the saved changes and shows what is published', async () => {
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    stub.metadata = metadataWith({ status: 'published', has_draft: true });
    stub.item = { title: 'Changed' };
    stub.publishedItem = { title: 'Hello' };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    await fresh.whenStable();

    discardButton(fresh.nativeElement).click();
    fresh.detectChanges();
    await fresh.whenStable();

    expect(confirmSpy).toHaveBeenCalledWith(
      'Discard the saved changes and go back to what is published? The site is not affected.',
    );
    expect(stub.discarded).toEqual([7]);
    // The item is read again, which is what puts the published content back on the screen.
    expect(stub.requested[stub.requested.length - 1]).toBe(7);
    expect(fresh.componentInstance.notice()).toEqual({ key: 'content.changesDiscarded' });
  });

  it('leaves the changes alone when the discard is declined', async () => {
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(false);
    stub.metadata = metadataWith({ status: 'published', has_draft: true });
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    await fresh.whenStable();

    discardButton(fresh.nativeElement).click();
    fresh.detectChanges();

    expect(confirmSpy).toHaveBeenCalled();
    expect(stub.discarded).toEqual([]);
    expect(stub.requested).toEqual([7]);
  });

  // Neither control is about an item that has nothing published to go back to, and neither is
  // offered to an account that may not edit this collection.
  it('offers no comparison or discard when there is nothing published', async () => {
    stub.metadata = metadataWith({ status: 'draft', has_draft: true });
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    await fresh.whenStable();

    expect(compareButton(fresh.nativeElement)).toBeUndefined();
    expect(discardButton(fresh.nativeElement)).toBeUndefined();
  });

  // A schema with previews turned off has no link to hand out, so the button is not offered at
  // all - an editor who cannot change the setting should not be shown a button that only refuses.
  it('does not offer a preview link when the schema does not allow one', async () => {
    // Before the component exists: it asks for the settings as it is built, not on first paint.
    stub.settings = { preview: false };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    await fresh.whenStable();

    expect(previewButton(fresh.nativeElement)).toBeUndefined();
  });

  // The other way round: the answer arrives after the form is on screen, and the button has to go.
  it('takes the preview link away when the setting arrives off', () => {
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();
    expect(previewButton(fresh.nativeElement)).toBeTruthy();

    stub.settings = { preview: false };
    fresh.componentInstance.previewAllowed.set(false);
    fresh.detectChanges();

    expect(previewButton(fresh.nativeElement)).toBeUndefined();
  });

  // A deployment with no preview site has nothing readable to hand over: the API's own answer is
  // JSON, so the screen says so rather than copying a link nobody can use.
  it('says so when the deployment has no preview site', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    try {
      capabilities.site = null;
      const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
      fresh.detectChanges();

      previewButton(fresh.nativeElement).click();
      await fresh.whenStable();
      fresh.detectChanges();

      expect(fresh.componentInstance.error()).toEqual(t('content.previewSiteNotConfigured'));
      expect(fresh.componentInstance.previewUrl()).toBe('');
      expect(fresh.componentInstance.notice()).toBeNull();
      expect(writeText).not.toHaveBeenCalled();
    } finally {
      delete (navigator as unknown as Record<string, unknown>)['clipboard'];
    }
  });

  // A link is minted for the item on screen when the button is pressed. Switching items reuses
  // this component, so the answer can arrive while another item is on screen - and a link for the
  // item the reader has left is neither theirs to show nor theirs to copy.
  it('ignores a preview link for the item the reader left', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    try {
      const held = new Subject<{ path: string; expires_at: string }>();
      stub.heldPreviews = held;
      const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
      fixture.detectChanges();
      const component = fixture.componentInstance;

      component.sharePreview();
      route.navigate({ name: 'blog', id: '9' });
      await fixture.whenStable();
      held.next({
        path: '/preview/collections/blog/items/7?token=1758000000.abc123',
        expires_at: '2026-09-13T12:00:00Z',
      });
      held.complete();
      await fixture.whenStable();
      fixture.detectChanges();

      expect(component.previewUrl()).toBe('');
      expect(component.notice()).toBeNull();
      expect(writeText).not.toHaveBeenCalled();
    } finally {
      // The other tests in this file rely on there being no clipboard in this environment.
      delete (navigator as unknown as Record<string, unknown>)['clipboard'];
    }
  });

  // The address moves to the item that was just created, and a route change may replace this
  // screen: the publish the button asked for has to be sent before that happens, not after.
  it('publishes what it created before the address moves to it', async () => {
    // Recorded when the call is subscribed to, the way a request is really sent: the stub's own
    // list is written when the observable is *built*, which is too early to notice a request that
    // never went out.
    const sent: { name: string; id: number }[] = [];
    vi.spyOn(stub, 'publishItem').mockImplementation((name: string, id: number) =>
      defer(() => {
        sent.push({ name, id });
        return of(stub.metadata);
      }),
    );
    const fixture: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fixture.detectChanges();
    const component = fixture.componentInstance;
    route.navigate({ name: 'blog' });
    await fixture.whenStable();
    // The route change that follows the save destroys this screen, which is what the ordering is
    // about: a navigation is free to do that, and this is the earliest it can happen.
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigate').mockImplementation(() => {
      fixture.destroy();
      return Promise.resolve(true);
    });
    component.setValue(
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      'New one',
    );

    component.saveAndPublish();
    await fixture.whenStable();

    expect(sent).toEqual([{ name: 'blog', id: 11 }]);
    expect(navigate).toHaveBeenCalled();
  });

  it('publishes the item it is editing without saving the form', () => {
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    const badge = fresh.nativeElement.querySelector('app-item-status .badge') as HTMLElement;
    expect(badge.textContent?.trim()).toBe('Draft');

    publishButton(fresh.nativeElement, 'Publish').click();
    fresh.detectChanges();

    expect(stub.published).toEqual([7]);
    expect(fresh.componentInstance.published()).toBe(true);
    expect(fresh.nativeElement.querySelector('app-item-status .badge')?.textContent?.trim()).toBe(
      'Published',
    );
    // The control now offers the opposite action.
    expect(publishButton(fresh.nativeElement, 'Unpublish')).toBeTruthy();
  });

  it('unpublishes an item that is currently published', () => {
    stub.metadata = {
      status: 'published',
      published_at: '2024-01-01T00:00:00Z',
      last_published_at: '2024-01-01T00:00:00Z',
      published_by: { id: 'u1', username: 'admin@example.com' },
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      has_draft: false,
    };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    // The editor names whoever published the version that is live, and labels the name: on its own
    // it read as the author, which it is not - `published_by` is the Account that *published*.
    const publisher = fresh.nativeElement.querySelector('.publisher') as HTMLElement;
    expect(publisher.textContent?.trim()).toBe('Published by: admin@example.com');
    expect(publisher.getAttribute('title')).toBe('Account that published this');

    publishButton(fresh.nativeElement, 'Unpublish').click();
    fresh.detectChanges();

    expect(stub.unpublished).toEqual([7]);
    expect(fresh.componentInstance.published()).toBe(false);
  });

  it('offers to release the changes waiting on a published item', () => {
    // The site is serving an older version. Without this the only way to release the changes was
    // to unpublish first, which takes the item off the site in the meantime.
    stub.metadata = {
      status: 'published',
      published_at: '2024-01-01T00:00:00Z',
      last_published_at: '2024-01-01T00:00:00Z',
      published_by: { id: 'u1', username: 'admin@example.com' },
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-02T00:00:00Z',
      has_draft: true,
    };
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    // Both acts are offered: releasing the changes, and taking the item down.
    expect(publishButton(fresh.nativeElement, 'Publish changes')).toBeTruthy();
    expect(publishButton(fresh.nativeElement, 'Unpublish')).toBeTruthy();

    publishButton(fresh.nativeElement, 'Publish changes').click();
    fresh.detectChanges();

    expect(stub.published).toEqual([7]);
    expect(stub.unpublished).toEqual([]);
    expect(fresh.componentInstance.published()).toBe(true);
    // Nothing is waiting any more, so the release button goes away.
    expect(hasButton(fresh.nativeElement, 'Publish changes')).toBe(false);
    expect(publishButton(fresh.nativeElement, 'Unpublish')).toBeTruthy();
  });
});

describe('CollectionItemEdit (new item)', () => {
  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [CollectionItemEdit],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        provideRouter([]),
        { provide: ActivatedRoute, useValue: stubActivatedRoute({ name: 'blog' }) },
        { provide: CollectionsService, useValue: new StubCollectionsService() },
      ],
    }).compileComponents();
  });

  /** There is no status to show until the item exists on the server. */
  it('offers no publish control before the item is saved', () => {
    const fresh: TypedFixture<CollectionItemEdit> = TestBed.createComponent(CollectionItemEdit);
    fresh.detectChanges();

    expect(fresh.componentInstance.isNew()).toBe(true);
    expect(fresh.nativeElement.querySelector('app-item-status')).toBeNull();
  });
});
