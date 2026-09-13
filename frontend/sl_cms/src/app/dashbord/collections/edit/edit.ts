import { Component, computed, inject, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';

import { AuthService } from 'app/core/auth/auth.service';
import { fieldCellStyle } from 'app/core/field-layout';
import { errorMessage as message } from 'app/core/http-error';
import { ItemMetadata } from 'app/models/item-status';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { CollectionValue } from 'app/models/values/collection';
import { FieldValue, withDefaults } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';
import { absolutePreviewUrl, copyToClipboard } from 'app/shared/preview-link';
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
  imports: [RouterLink, MatButtonModule, ItemStatusBadge, ValueField],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class Edit {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private collectionsService = inject(CollectionsService);
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
  public error = signal('');
  /** Draft/published state; `null` for an item that has not been saved yet. */
  public metadata = signal<ItemMetadata | null>(null);
  public published = computed(() => this.metadata()?.status === 'published');
  /** The shareable preview link, once one has been minted. */
  public previewUrl = signal('');
  /** What happened to the preview link: copied, or made but not copied. */
  public notice = signal('');
  /** Places each field on the shared 12-column grid, mirroring the schema editor. */
  public cellStyle = fieldCellStyle;

  /**
   * Per-field input problems reported by the value fields (invalid JSON, failed upload).
   * Saving is refused while any remain, so bad input is neither sent nor replaced by a
   * stale value without the user noticing.
   */
  private fieldErrors: { [field: string]: string } = {};

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
      error: (e) => this.error.set(`Failed to load the schema: ${message(e)}`),
    });
  }

  private loadItem(schema: CollectionSchema, id: number) {
    this.collectionsService.getCollectionItem(this.collectionName, id).subscribe({
      next: (values) => this.values.set(withDefaults(schema, values)),
      error: (e) => this.error.set(`Failed to load the item: ${message(e)}`),
    });
  }

  private loadMetadata(id: number) {
    this.collectionsService.getItemMetadata(this.collectionName, id).subscribe({
      next: (metadata) => this.metadata.set(metadata),
      error: (e) => this.error.set(`Failed to load the published state: ${message(e)}`),
    });
  }

  /** Publish or unpublish without saving the form (the two are independent acts). */
  togglePublished() {
    const id = this.itemId;
    if (id === null) {
      return;
    }

    const request = this.published()
      ? this.collectionsService.unpublishItem(this.collectionName, id)
      : this.collectionsService.publishItem(this.collectionName, id);

    request.subscribe({
      next: (metadata) => {
        this.error.set('');
        this.metadata.set(metadata);
      },
      error: (e) => this.error.set(`Could not change the published state: ${message(e)}`),
    });
  }

  setValue(field: FieldSchema, value: FieldValue) {
    // Replaced rather than mutated: the template reads the signal, and changing the object
    // inside it would not tell Angular anything happened.
    this.values.update((values) => ({ ...values, [field.name]: value }));
  }

  setFieldError(field: FieldSchema, message: string | null) {
    if (message) {
      this.fieldErrors[field.name] = message;
    } else {
      delete this.fieldErrors[field.name];
    }
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
    this.error.set('');
    this.notice.set('');
    this.collectionsService.createPreviewLink(this.collectionName, this.itemId as number).subscribe({
      next: async (link) => {
        const url = absolutePreviewUrl(link.path);
        this.previewUrl.set(url);
        const copied = await copyToClipboard(url);
        const expires = new Date(link.expires_at).toLocaleString();
        this.notice.set(
          copied
            ? `プレビュー URL をコピーしました(有効期限: ${expires})`
            : `プレビュー URL を作成しました(有効期限: ${expires})。コピーできなかったので下から手動でコピーしてください`,
        );
      },
      error: (e) => this.error.set(`Could not create a preview link: ${message(e)}`),
    });
  }

  save() {
    const problems = Object.values(this.fieldErrors);
    if (problems.length > 0) {
      this.error.set(problems[0]);
      return;
    }

    this.error.set('');
    const values: CollectionValue = { ...this.values() };

    // Subscribe per branch: the create and update calls return different observable
    // types, which cannot be unioned into a single `subscribe` call.
    const id = this.itemId;
    if (id === null) {
      this.collectionsService.createCollectionItem(this.collectionName, values).subscribe({
        next: () => this.goBackToList(),
        error: (e) => this.error.set(`Save failed: ${message(e)}`),
      });
    } else {
      this.collectionsService.updateCollectionItem(this.collectionName, id, values).subscribe({
        next: () => this.goBackToList(),
        error: (e) => this.error.set(`Save failed: ${message(e)}`),
      });
    }
  }

  private goBackToList() {
    this.router.navigate(['/collections', this.collectionName]);
  }
}
