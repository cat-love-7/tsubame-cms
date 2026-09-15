import { Component, computed, inject, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { TranslocoPipe } from '@jsverse/transloco';

import { AuthService } from 'app/core/auth/auth.service';
import { fieldCellStyle } from 'app/core/field-layout';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe, failure, fieldOf, t } from 'app/core/i18n/message';
import { ItemMetadata } from 'app/models/item-status';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { CollectionValue } from 'app/models/values/collection';
import { FieldValue, withDefaults } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';
import { absoluteApiUrl, copyToClipboard } from 'app/shared/share-link';
import { ValueField } from 'app/shared/value-field/value-field';

/**
 * Create/edit one collection item.
 *
 * The form is driven by the collection schema: the shared `ValueField` renders the widget
 * each field type needs and owns its own input state. Values are sent **without type
 * tags**, and fields this editor cannot edit yet (composite fields) keep whatever the
 * server sent so saving never silently discards them.
 *
 * Publishing is separate from saving: an item may be edited any number of times while it
 * stays a draft, and only publishing makes it visible to the public content API.
 */
@Component({
  selector: 'app-item-edit',
  imports: [ItemStatusBadge, MatButtonModule, MessagePipe, RouterLink, TranslocoPipe, ValueField],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class Edit {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private collectionsService = inject(CollectionsService);
  private dates = inject(DateTimeFormat);
  /** A read-only account sees the form but cannot change it. */
  public auth = inject(AuthService);

  public collectionName: string = this.route.snapshot.params['name'];
  private readonly itemId: number | null =
    this.route.snapshot.params['id'] === undefined
      ? null
      : Number(this.route.snapshot.params['id']);
  public readonly isNew = this.itemId === null;

  /**
   * The schema and the values being edited, as signals.
   *
   * They arrive from two asynchronous loads, and a plain field assigned from a response
   * can land while Angular is checking the view: dev mode then reported
   * `ExpressionChangedAfterItHasBeenCheckedError` for `schema.length === 0` every time an
   * item was opened. A signal tells Angular the view changed instead of tripping over it.
   */
  public schema = signal<CollectionSchema>([]);
  public values = signal<CollectionValue>({});
  /** The failure to show, as a key or as the server's own words. */
  public error = signal<Message | null>(null);
  /** The field the last refusal was about, so the form can mark that one input. */
  public problemField = signal<string | null>(null);
  /** Draft/published state; `null` for an item that has not been saved yet. */
  public metadata = signal<ItemMetadata | null>(null);
  public published = computed(() => this.metadata()?.status === 'published');
  /** A published item with an unpublished working copy: the site is behind the editor. */
  public hasDraft = computed(() => this.metadata()?.has_draft ?? false);
  /** What this account may do *with this collection*, overrides included. */
  public canEdit = computed(() => this.auth.canEditIn('collections', this.collectionName));
  public canPublish = computed(() => this.auth.canPublishIn('collections', this.collectionName));
  /** The shareable preview link, once one has been minted. */
  public previewUrl = signal('');
  /** What happened to the preview link: copied, or made but not copied. */
  public notice = signal<Message | null>(null);
  /** Places each field on the shared 12-column grid, mirroring the schema editor. */
  public cellStyle = fieldCellStyle;

  /**
   * Per-field input problems reported by the value fields (invalid JSON, failed upload).
   * Saving is refused while any remain, so bad input is neither sent nor replaced by a
   * stale value without the user noticing.
   */
  private fieldErrors: { [field: string]: Message } = {};

  constructor() {
    this.collectionsService.getCollectionSchema(this.collectionName).subscribe({
      next: (schema) => {
        this.schema.set(schema);
        if (this.itemId === null) {
          this.values.set(withDefaults(schema, {}));
        } else {
          this.loadItem(schema, this.itemId);
          this.loadMetadata(this.itemId);
        }
      },
      error: (e) => this.error.set(failure('content.failedToLoadSchema', e)),
    });
  }

  private loadItem(schema: CollectionSchema, id: number) {
    this.collectionsService.getCollectionItem(this.collectionName, id).subscribe({
      next: (values) => this.values.set(withDefaults(schema, values)),
      error: (e) => this.error.set(failure('content.failedToLoadItem', e)),
    });
  }

  private loadMetadata(id: number) {
    this.collectionsService.getItemMetadata(this.collectionName, id).subscribe({
      next: (metadata) => this.metadata.set(metadata),
      error: (e) => this.error.set(failure('content.failedToLoadPublishedState', e)),
    });
  }

  /**
   * Publish, or release the changes waiting on a published item.
   *
   * The server treats publishing as the copy: it replaces the published item with the working
   * copy. Doing it again on a published item is therefore exactly "make the site match the
   * editor" - there is no need to take the item down first.
   */
  publish() {
    this.setPublished(true);
  }

  /** Take the item off the site. Its working copy is kept. */
  unpublish() {
    this.setPublished(false);
  }

  private setPublished(published: boolean) {
    const id = this.itemId;
    if (id === null) {
      return;
    }

    const request = published
      ? this.collectionsService.publishItem(this.collectionName, id)
      : this.collectionsService.unpublishItem(this.collectionName, id);

    request.subscribe({
      next: (metadata) => {
        this.error.set(null);
        this.metadata.set(metadata);
      },
      error: (e) => this.error.set(failure('content.failedToChangePublished', e)),
    });
  }

  setValue(field: FieldSchema, value: FieldValue) {
    // Replaced rather than mutated: the template reads the signal, and changing the object
    // inside it would not tell Angular anything happened.
    this.values.update((values) => ({ ...values, [field.name]: value }));
  }

  setFieldError(field: FieldSchema, message: Message | null) {
    if (message) {
      this.fieldErrors[field.name] = message;
      // A problem the widget itself found (a text outside its lengths, unparseable JSON) marks
      // its input exactly like a refusal from the server does.
      this.problemField.set(field.name);
    } else {
      delete this.fieldErrors[field.name];
      if (this.problemField() === field.name) {
        this.problemField.set(null);
      }
    }
  }

  /**
   * Whether this cell is the one a refusal named.
   *
   * The server names the input as a path (`title`, `tags[2]`, `seo.description`), so a refusal
   * about something inside a composite still marks the composite's cell rather than nothing.
   */
  isProblem(field: FieldSchema): boolean {
    const problem = this.problemField();
    if (problem === null) {
      return false;
    }
    return (
      problem === field.name ||
      problem.startsWith(`${field.name}.`) ||
      problem.startsWith(`${field.name}[`)
    );
  }

  /**
   * Mint a link that shows this working copy to someone without an account, and copy it.
   *
   * Saving first is not required: the link always shows what is stored, which is what a
   * reviewer should be looking at anyway.
   */
  sharePreview() {
    if (this.isNew) {
      return;
    }
    this.error.set(null);
    this.notice.set(null);
    this.collectionsService.createPreviewLink(this.collectionName, this.itemId as number).subscribe({
      next: async (link) => {
        const url = absoluteApiUrl(link.path);
        this.previewUrl.set(url);
        const copied = await copyToClipboard(url);
        const expires = this.dates.format(link.expires_at);
        this.notice.set(
          copied
            ? t('content.previewCopied', { expires })
            : t('content.previewNotCopied', { expires }),
        );
      },
      error: (e) => this.error.set(failure('content.failedToCreatePreviewLink', e)),
    });
  }

  save() {
    const problems = Object.values(this.fieldErrors);
    if (problems.length > 0) {
      this.error.set(problems[0]);
      return;
    }

    this.error.set(null);
    this.problemField.set(null);
    const values: CollectionValue = { ...this.values() };

    // Subscribe per branch: the create and update calls return different observable
    // types, which cannot be unioned into a single `subscribe` call.
    const id = this.itemId;
    if (id === null) {
      this.collectionsService.createCollectionItem(this.collectionName, values).subscribe({
        next: () => this.goBackToList(),
        error: (e) => this.refuse(e),
      });
    } else {
      this.collectionsService.updateCollectionItem(this.collectionName, id, values).subscribe({
        next: () => this.goBackToList(),
        error: (e) => this.refuse(e),
      });
    }
  }

  /** A save the server refused: remember which field it was about, and mark it. */
  private refuse(error: unknown) {
    this.problemField.set(fieldOf(error));
    this.error.set(failure('content.saveFailed', error));
  }

  private goBackToList() {
    this.router.navigate(['/collections', this.collectionName]);
  }
}
