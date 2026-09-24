import {
  Component,
  DestroyRef,
  HostListener,
  WritableSignal,
  computed,
  inject,
  signal,
} from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';
import { map, of } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { fieldCellStyle } from 'app/core/field-layout';
import { HasUnsavedChanges } from 'app/core/unsaved-changes.guard';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe } from 'app/core/i18n/message';
import { FieldSchema } from 'app/models/schema/fields';
import { FieldValue } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';
import { RelationReferences } from 'app/shared/relation-references/relation-references';
import { ValueField } from 'app/shared/value-field/value-field';
import { NoticeToast } from 'app/shared/notice-toast/notice-toast';

import { ContentEditor, ContentSource } from '../../shared/content-editor/content-editor';

/**
 * Create/edit one collection item.
 *
 * The form is driven by the collection schema: the shared `ValueField` renders the widget each
 * field type needs and owns its own input state. Values are sent **without type tags**, and fields
 * this editor cannot edit (composite fields) keep whatever the server sent, so saving never
 * silently discards them.
 *
 * The screen itself is thin: everything it does to the item (load it, save it - creating it on the
 * first save - publish it, compare it with what is live, discard the working copy, mint a preview
 * link) is [`ContentEditor`], which the single-page editor uses too. What is here is what makes
 * this an *item*: an id that only exists after the first save, an address that moves to it, and the
 * list this screen came from.
 */
@Component({
  selector: 'app-collection-item-edit',
  imports: [
    ItemStatusBadge,
    MatButtonModule,
    MessagePipe,
    NoticeToast,
    RelationReferences,
    RouterLink,
    TranslocoPipe,
    ValueField,
  ],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class CollectionItemEdit implements HasUnsavedChanges {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private destroyRef = inject(DestroyRef);
  private collectionsService = inject(CollectionsService);
  private i18n = inject(TranslocoService);
  private capabilities = inject(CapabilitiesService);
  private dates = inject(DateTimeFormat);
  /** A read-only account sees the form but cannot change it. */
  public auth = inject(AuthService);
  /** Places each field on the shared 12-column grid, mirroring the schema editor. */
  public cellStyle = fieldCellStyle;

  private itemId = signal<number | null>(null);
  /**
   * The id the address names, which stops matching the item once a new one has been created.
   *
   * A create answers a new id, and the address is moved to it without a reload; a second create
   * from the same screen would otherwise be mistaken for the item it just made.
   */
  private routeItemId = signal<number | null>(null);
  public isNew = computed(() => this.itemId() === null);

  /**
   * The screen's own acts and state, over this item.
   *
   * The source reads the collection name and the id when it is *called* rather than capturing them,
   * so a switch that happens while a request is in flight is what the answer is judged against. A
   * new item has no values and no status to read: it starts from the schema's defaults, and the
   * metadata stays absent until the first save.
   */
  private readonly editor = new ContentEditor(
    {
      // "new" for an item that does not exist yet, so the address changes when a create answers.
      address: () => `${this.collectionName()}/${this.itemId() ?? 'new'}`,
      exists: () => this.itemId() !== null,
      create: (values) =>
        this.collectionsService.createCollectionItem(this.collectionName(), values),
      created: (id) => this.itemId.set(id),
      update: (values) =>
        this.collectionsService.updateCollectionItem(this.collectionName(), this.itemId()!, values),
      loadSchema: () => this.collectionsService.getCollectionSchema(this.collectionName()),
      loadValues: () =>
        this.itemId() === null
          ? of({})
          : this.collectionsService.getCollectionItem(this.collectionName(), this.itemId()!),
      loadMetadata: () =>
        this.itemId() === null
          ? of(null)
          : this.collectionsService.getItemMetadata(this.collectionName(), this.itemId()!),
      loadPublishedValues: () =>
        this.collectionsService.getPublishedItem(this.collectionName(), this.itemId()!),
      loadPreviewAllowed: () =>
        this.collectionsService
          .getCollectionSettings(this.collectionName())
          .pipe(map((settings) => settings.preview)),
      setPublished: (published) =>
        published
          ? this.collectionsService.publishItem(this.collectionName(), this.itemId()!)
          : this.collectionsService.unpublishItem(this.collectionName(), this.itemId()!),
      discardDraft: () =>
        this.collectionsService.discardItemDraft(this.collectionName(), this.itemId()!),
      createPreviewLink: () =>
        this.collectionsService.createPreviewLink(this.collectionName(), this.itemId()!),
    } satisfies ContentSource,
    {
      i18n: this.i18n,
      capabilities: this.capabilities,
      dates: this.dates,
      destroyRef: this.destroyRef,
    },
  );

  /**
   * The collection being edited.
   *
   * The editor's own signal, exposed under the name this screen and its template use. The sidebar
   * and the item list reuse this component, so the parameters are read from the stream in the
   * constructor rather than once here.
   */
  public collectionName: WritableSignal<string> = this.editor.name;
  public schema = this.editor.schema;
  public values = this.editor.values;
  public error = this.editor.error;
  public problemField = this.editor.problemField;
  public metadata = this.editor.metadata;
  public published = this.editor.published;
  public hasDraft = this.editor.hasDraft;
  public loaded = this.editor.loaded;
  public unsavedChanges = this.editor.unsavedChanges;
  public previewAllowed = this.editor.previewAllowed;
  public previewUrl = this.editor.previewUrl;
  public notice = this.editor.notice;
  public publishedValues = this.editor.publishedValues;
  public comparing = this.editor.comparing;
  public changedFields = this.editor.changedFields;

  /** What this account may do *with this collection*, overrides included. */
  public canEdit = computed(() => this.auth.canEditIn('collections', this.collectionName()));
  public canPublish = computed(() => this.auth.canPublishIn('collections', this.collectionName()));
  /** The item the references panel is about: what this screen is editing, once it exists. */
  public referencedItemId = computed(() => this.itemId());

  constructor() {
    // A destroyed screen is nobody's screen: the editor lets go of everything in flight, so a save
    // that landed after the reader had gone elsewhere does not pass the guard and take them back to
    // the list the save was pressed from.
    this.destroyRef.onDestroy(() => this.editor.destroy());
    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const name = params.get('name') ?? '';
      const id = params.get('id') === null ? null : Number(params.get('id'));
      this.routeItemId.set(id);
      if (name !== this.collectionName() || id !== this.itemId()) {
        this.load(name, id);
      }
    });
  }

  /** Everything the form shows belongs to one item, so a switch starts from nothing. */
  private load(name: string, id: number | null) {
    this.itemId.set(id);
    this.editor.load(name);
  }

  setValue(field: FieldSchema, value: FieldValue) {
    this.editor.setValue(field, value);
  }

  setFieldError(field: FieldSchema, message: Message | null) {
    this.editor.setFieldError(field, message);
  }

  isProblem(field: FieldSchema): boolean {
    return this.editor.isProblem(field);
  }

  /**
   * Save the working copy, and go back to the list.
   *
   * The save was pressed from the list, so that is where the reader is going; a save that answers
   * after they have moved on is dropped rather than navigating them from wherever they are.
   */
  save() {
    this.editor.save(() => this.goBackToList());
  }

  saveAndPublish() {
    this.editor.saveAndPublish(() => this.addressTheNewItem());
  }

  publish() {
    this.editor.publish();
  }

  unpublish() {
    this.editor.unpublish();
  }

  compareWithPublished() {
    this.editor.compareWithPublished();
  }

  discardChanges() {
    this.editor.discardChanges();
  }

  sharePreview() {
    this.editor.sharePreview();
  }

  retry() {
    this.editor.reload();
  }

  /** Whether the form holds edits that would be lost by leaving. */
  hasUnsavedChanges(): boolean {
    return this.editor.hasUnsavedChanges();
  }

  /** Warn before a reload or a closed tab, which no route guard can see. */
  @HostListener('window:beforeunload', ['$event'])
  warnBeforeLeaving(event: BeforeUnloadEvent) {
    if (this.unsavedChanges()) {
      event.preventDefault();
    }
  }

  /**
   * Move the address to the item that was just created.
   *
   * Only for a create: an existing item already has its address, and replacing it would throw away
   * the history entry the reader came from.
   */
  private addressTheNewItem() {
    if (this.routeItemId() !== null) {
      return;
    }
    void this.router.navigate(['/collections', this.collectionName(), 'edit', this.itemId()], {
      replaceUrl: true,
    });
  }

  private goBackToList() {
    void this.router.navigate(['/collections', this.collectionName()]);
  }
}
