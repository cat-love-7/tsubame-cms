import { Component, computed, inject, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';

import { AuthService } from 'app/core/auth/auth.service';
import { fieldCellStyle } from 'app/core/field-layout';
import { errorMessage as message } from 'app/core/http-error';
import { ItemMetadata } from 'app/models/item-status';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { ContentValue } from 'app/models/values/collection';
import { FieldValue, withDefaults } from 'app/models/values/fields';
import { SinglePagesService } from 'app/services/schema/single_pages.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';
import { absoluteApiUrl, copyToClipboard } from 'app/shared/share-link';
import { ValueField } from 'app/shared/value-field/value-field';

/**
 * Edit the content of one single page.
 *
 * A single page has exactly one item, so there is no list and no create/delete here —
 * only the form. It shares `ValueField` and the layout grid with the collection editor,
 * and publishing is a separate act from saving just as it is for a collection item.
 */
@Component({
  selector: 'app-single-page-edit',
  imports: [RouterLink, MatButtonModule, ItemStatusBadge, ValueField],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class Edit {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private pages = inject(SinglePagesService);
  /** A read-only account sees the form but cannot change it. */
  public auth = inject(AuthService);

  public pageName: string = this.route.snapshot.params['name'];
  /** Signals, for the reason given in the collection item editor: both arrive from
   * asynchronous loads that would otherwise trip the dev-mode change check. */
  public schema = signal<CollectionSchema>([]);
  public values = signal<ContentValue>({});
  public error = signal('');
  public metadata = signal<ItemMetadata | null>(null);
  public published = computed(() => this.metadata()?.status === 'published');
  /** What this account may do *with this page*, overrides included. */
  public canEdit = computed(() => this.auth.canEditIn('single_pages', this.pageName));
  public canPublish = computed(() => this.auth.canPublishIn('single_pages', this.pageName));
  /** The shareable preview link, once one has been minted. */
  public previewUrl = signal('');
  /** What happened to the preview link: copied, or made but not copied. */
  public notice = signal('');
  public cellStyle = fieldCellStyle;

  /** Per-field problems reported by the value fields; saving is refused while any remain. */
  private fieldErrors: { [field: string]: string } = {};

  constructor() {
    this.pages.getPageSchema(this.pageName).subscribe({
      next: (schema) => {
        this.schema.set(schema);
        this.loadItem(schema);
      },
      error: (e) => this.error.set(`Failed to load the schema: ${message(e)}`),
    });

    this.pages.getPageMetadata(this.pageName).subscribe({
      next: (metadata) => this.metadata.set(metadata),
      error: (e) => this.error.set(`Failed to load the published state: ${message(e)}`),
    });
  }

  private loadItem(schema: CollectionSchema) {
    this.pages.getPageItem(this.pageName).subscribe({
      next: (values) => this.values.set(withDefaults(schema, values)),
      error: (e) => this.error.set(`Failed to load the content: ${message(e)}`),
    });
  }

  /** Publish or unpublish without saving the form (the two are independent acts). */
  togglePublished() {
    const request = this.published()
      ? this.pages.unpublishPage(this.pageName)
      : this.pages.publishPage(this.pageName);

    request.subscribe({
      next: (metadata) => {
        this.error.set('');
        this.metadata.set(metadata);
      },
      error: (e) => this.error.set(`Could not change the published state: ${message(e)}`),
    });
  }

  setValue(field: FieldSchema, value: FieldValue) {
    // Replaced rather than mutated: the template reads the signal.
    this.values.update((values) => ({ ...values, [field.name]: value }));
  }

  setFieldError(field: FieldSchema, message: string | null) {
    if (message) {
      this.fieldErrors[field.name] = message;
    } else {
      delete this.fieldErrors[field.name];
    }
  }

  /** Mint a link that shows this working copy to someone without an account, and copy it
   * (see the collection item editor for the reasoning). */
  sharePreview() {
    this.error.set('');
    this.notice.set('');
    this.pages.createPreviewLink(this.pageName).subscribe({
      next: async (link) => {
        const url = absoluteApiUrl(link.path);
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
    this.pages.updatePageItem(this.pageName, { ...this.values() }).subscribe({
      next: () => {
        this.error.set('');
        this.router.navigate(['/settings/single-pages']);
      },
      error: (e) => this.error.set(`Save failed: ${message(e)}`),
    });
  }
}
