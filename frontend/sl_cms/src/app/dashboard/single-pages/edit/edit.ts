import {
  Component,
  DestroyRef,
  HostListener,
  WritableSignal,
  computed,
  inject,
} from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';
import { map } from 'rxjs';

import { AuthService } from 'app/core/auth/auth.service';
import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { fieldCellStyle } from 'app/core/field-layout';
import { HasUnsavedChanges } from 'app/core/unsaved-changes.guard';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe } from 'app/core/i18n/message';
import { FieldSchema } from 'app/models/schema/fields';
import { FieldValue } from 'app/models/values/fields';
import { SinglePagesService } from 'app/services/schema/single-pages.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';
import { RelationReferences } from 'app/shared/relation-references/relation-references';
import { ValueField } from 'app/shared/value-field/value-field';
import { NoticeToast } from 'app/shared/notice-toast/notice-toast';

import { ContentEditor, ContentSource } from '../../shared/content-editor/content-editor';

/**
 * Edit the content of one single page.
 *
 * A single page has exactly one item, so there is no list and no create/delete here — only the
 * form. The screen itself is thin: everything it does to the content (load it, save it, publish
 * it, compare it with what is live, discard the working copy, mint a preview link) is
 * [`ContentEditor`], which the collection item editor uses too. What is here is what makes this a
 * *page*: the name in the address, and which service the acts go to.
 */
@Component({
  selector: 'app-single-page-edit',
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
export class SinglePageEdit implements HasUnsavedChanges {
  private route = inject(ActivatedRoute);
  private destroyRef = inject(DestroyRef);
  private pages = inject(SinglePagesService);
  private i18n = inject(TranslocoService);
  private capabilities = inject(CapabilitiesService);
  private dates = inject(DateTimeFormat);
  /** A read-only account sees the form but cannot change it. */
  public auth = inject(AuthService);

  /**
   * The screen's own acts and state, over this page.
   *
   * The source reads the page name when it is *called* rather than capturing it, so a switch that
   * happens while a request is in flight is what the answer is judged against.
   */
  private readonly editor = new ContentEditor(
    {
      address: () => this.pageName(),
      // A page exists as soon as it has a schema, so a save never creates one and there is no
      // `create` here (see `ContentSource.exists`).
      exists: () => true,
      update: (values) => this.pages.updatePageItem(this.pageName(), values),
      loadSchema: () => this.pages.getPageSchema(this.pageName()),
      loadValues: () => this.pages.getPageItem(this.pageName()),
      loadMetadata: () => this.pages.getPageMetadata(this.pageName()),
      loadPublishedValues: () => this.pages.getPublishedPageItem(this.pageName()),
      loadPreviewAllowed: () =>
        this.pages.getPageSettings(this.pageName()).pipe(map((settings) => settings.preview)),
      setPublished: (published) =>
        published
          ? this.pages.publishPage(this.pageName())
          : this.pages.unpublishPage(this.pageName()),
      discardDraft: () => this.pages.discardPageDraft(this.pageName()),
      createPreviewLink: () => this.pages.createPreviewLink(this.pageName()),
    } satisfies ContentSource,
    {
      i18n: this.i18n,
      capabilities: this.capabilities,
      dates: this.dates,
      destroyRef: this.destroyRef,
    },
  );

  /**
   * The page being edited.
   *
   * The editor's own signal, exposed under the name this screen and its template use. The sidebar
   * switches pages without leaving this route and the router reuses the component, so the name is
   * read from the parameter stream in the constructor rather than once here.
   */
  public pageName: WritableSignal<string> = this.editor.name;
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

  /** What this account may do *with this page*, overrides included. */
  public canEdit = computed(() => this.auth.canEditIn('single_pages', this.pageName()));
  public canPublish = computed(() => this.auth.canPublishIn('single_pages', this.pageName()));
  /** Places each field on the shared 12-column grid, mirroring the schema editor. */
  public cellStyle = fieldCellStyle;

  constructor() {
    // A destroyed screen is nobody's screen: the editor lets go of everything in flight, so a save
    // that landed after the reader had gone elsewhere does not touch what is on screen now.
    this.destroyRef.onDestroy(() => this.editor.destroy());
    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const name = params.get('name') ?? '';
      if (name !== this.pageName()) {
        this.editor.load(name);
      }
    });
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

  save() {
    this.editor.save();
  }

  saveAndPublish() {
    this.editor.saveAndPublish();
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
}
