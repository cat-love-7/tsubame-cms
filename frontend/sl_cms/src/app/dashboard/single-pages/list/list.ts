import { Component, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe } from '@jsverse/transloco';

import { AuthService } from 'app/core/auth/auth.service';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe, failure } from 'app/core/i18n/message';
import { ItemMetadata, ItemStatus } from 'app/models/item-status';
import { SinglePagesService } from 'app/services/schema/single_pages.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';

/**
 * Every single page, with the state of each.
 *
 * A single page is one item, so this is not a content list the way a collection is: it is the
 * overview an editor needs to see what is live, what has unpublished changes, and who released
 * it - and to publish from here without opening the page.
 */
@Component({
  selector: 'app-single-page-list',
  imports: [ MatTooltipModule,ItemStatusBadge, MatButtonModule, MatIconModule, MessagePipe, RouterLink, TranslocoPipe],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class SinglePageList {
  private pages = inject(SinglePagesService);
  private dates = inject(DateTimeFormat);
  /** What the signed-in account may do; the server enforces the same rules. */
  public auth = inject(AuthService);

  public names = signal<string[]>([]);
  /** State per page name; a page that has never been published has no stored record. */
  public metadata = signal<{ [name: string]: ItemMetadata }>({});
  public error = signal<Message | null>(null);

  public empty = computed(() => this.names().length === 0);

  constructor() {
    this.load();
  }

  private load() {
    this.pages.listPageNames().subscribe({
      next: (names) => this.names.set(names),
      error: (e) => this.error.set(failure('content.failedToLoadPages', e)),
    });
    this.pages.listItemMetadata().subscribe({
      next: (metadata) => this.metadata.set(metadata),
      error: (e) => this.error.set(failure('content.failedToLoadPublishedState', e)),
    });
  }

  statusOf(name: string): ItemStatus {
    return this.metadata()[name]?.status ?? 'draft';
  }

  hasDraft(name: string): boolean {
    return this.metadata()[name]?.has_draft ?? false;
  }

  publishedBy(name: string): string {
    return this.metadata()[name]?.published_by?.username ?? '';
  }

  updatedAt(name: string): string {
    return this.dates.format(this.metadata()[name]?.updated_at);
  }

  canEdit(name: string): boolean {
    return this.auth.canEditIn('single_pages', name);
  }

  canPublish(name: string): boolean {
    return this.auth.canPublishIn('single_pages', name);
  }

  /** Publish a draft, or release the changes waiting on a published page. */
  publish(name: string) {
    this.setPublished(name, true);
  }

  unpublish(name: string) {
    this.setPublished(name, false);
  }

  private setPublished(name: string, published: boolean) {
    const request = published
      ? this.pages.publishPage(name)
      : this.pages.unpublishPage(name);
    request.subscribe({
      next: (metadata) => {
        this.error.set(null);
        this.metadata.update((all) => ({ ...all, [name]: metadata }));
      },
      error: (e) => this.error.set(failure('content.failedToChangePublished', e)),
    });
  }
}
