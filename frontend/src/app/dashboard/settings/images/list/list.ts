import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';

import { AuthService } from 'app/core/auth/auth.service';
import { apiUrl } from 'app/core/api-url';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { ImageEntry, ImageOwner } from 'app/repositories/media/images.repository';
import { IMAGE_PAGE_SIZE, ImagesService } from 'app/services/media/images.service';
import { Observable, forkJoin } from 'rxjs';
import { NoticeToast } from 'app/shared/notice-toast/notice-toast';
import { absoluteApiUrl, copyToClipboard } from 'app/shared/share-link';

/**
 * The image library.
 *
 * Shows everything that has been uploaded, newest first, and does the two things an editor
 * needs from it: add an image, and remove one. The content forms pick from this same list,
 * so an image uploaded once can be reused instead of uploaded again.
 */
@Component({
  selector: 'app-image-library',
  imports: [
    MatTooltipModule,
    FormsModule,
    MatButtonModule,
    MatIconModule,
    MessagePipe,
    TranslocoPipe,
    NoticeToast,
  ],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class ImageLibrary {
  private images = inject(ImagesService);
  private i18n = inject(TranslocoService);
  private dates = inject(DateTimeFormat);
  /** Uploading is open to anyone who may edit content; changing one is not. */
  public auth = inject(AuthService);

  public library = signal<ImageEntry[]>([]);
  /** How many images there are altogether, which is more than are on screen (see `loadMore`). */
  public libraryTotal = signal(0);
  /** What has been taken out of the library, and is waiting to be put back or deleted for good. */
  public trashed = signal<ImageEntry[]>([]);
  public trashTotal = signal(0);
  /** Whether another page is on its way, so the button can say so. */
  public loadingMore = signal(false);
  /** Whether the screen is showing the trash. */
  public showTrash = signal(false);
  public error = signal<Message | null>(null);
  public uploading = signal(false);
  /** The image whose name is being edited, and what has been typed so far. */
  public renaming = signal<number | null>(null);
  public draftName = signal('');
  /** What just happened (a link copied), as a key or the server's own words. */
  public notice = signal<Message | null>(null);
  /** Exposed for the template. */
  public imageUrl = apiUrl;

  constructor() {
    this.load();
  }

  /**
   * How many pages of each list the screen is holding. One until the reader asks for more.
   *
   * Pages rather than a count, because what the screen re-reads after an image moves is "as much
   * as it already had" - an editor who scrolled four pages down should not be sent back to the
   * first one by deleting something.
   */
  private loadedPages = 1;

  /**
   * Both lists, so the toggle is instant and the trash button can say how much is in it.
   *
   * Each is one request per loaded page, and the two are asked for together: a screen that had to
   * fetch on every switch would make the simple act of looking in the trash feel like a page
   * load. The whole library is *not* read: a page is what the grid renders, and the rest arrives
   * when the reader asks (`loadMore`).
   */
  private load() {
    const offsets = Array.from(
      { length: Math.max(1, this.loadedPages) },
      (_, index) => index * IMAGE_PAGE_SIZE,
    );
    forkJoin(offsets.map((offset) => this.images.listImages(offset))).subscribe({
      next: (pages) => {
        this.library.set(pages.flatMap((page) => page.images));
        this.libraryTotal.set(pages[pages.length - 1]?.total ?? 0);
      },
      error: (e) => this.error.set(failure('content.failedToLoadImages', e)),
    });
    forkJoin(offsets.map((offset) => this.images.listTrash(offset))).subscribe({
      next: (pages) => {
        this.trashed.set(pages.flatMap((page) => page.images));
        this.trashTotal.set(pages[pages.length - 1]?.total ?? 0);
      },
      error: (e) => this.error.set(failure('content.failedToLoadImages', e)),
    });
  }

  /** Ask for the next page of what is on screen. */
  loadMore() {
    if (this.loadingMore() || !this.canLoadMore()) {
      return;
    }
    this.loadingMore.set(true);
    this.loadedPages += 1;
    const showTrash = this.showTrash();
    const offset = (showTrash ? this.trashed() : this.library()).length;
    const next = showTrash ? this.images.listTrash(offset) : this.images.listImages(offset);
    next.subscribe({
      next: (page) => {
        this.loadingMore.set(false);
        if (showTrash) {
          this.trashed.update((images) => [...images, ...page.images]);
          this.trashTotal.set(page.total);
        } else {
          this.library.update((images) => [...images, ...page.images]);
          this.libraryTotal.set(page.total);
        }
      },
      error: (e) => {
        // The page did not arrive, so the screen is still holding the one before it.
        this.loadingMore.set(false);
        this.loadedPages -= 1;
        this.error.set(failure('content.failedToLoadImages', e));
      },
    });
  }

  /** Whether there is more of the list on screen than it has been handed. */
  public canLoadMore = () => this.visible().length < this.visibleTotal();

  /** How many images the list on screen holds altogether. */
  public visibleTotal = () => (this.showTrash() ? this.trashTotal() : this.libraryTotal());

  /** Look at the library, or at what has been taken out of it. */
  showLibrary(showTrash: boolean) {
    this.showTrash.set(showTrash);
  }

  /** The list the screen is showing. */
  public visible = () => (this.showTrash() ? this.trashed() : this.library());

  onFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) {
      return;
    }
    this.uploading.set(true);
    this.error.set(null);
    this.images.uploadImage(file).subscribe({
      next: () => {
        this.uploading.set(false);
        // Clear the input so picking the same file again still fires a change event.
        input.value = '';
        this.load();
      },
      error: (e) => {
        this.uploading.set(false);
        input.value = '';
        this.error.set(failure('content.uploadFailed', e));
      },
    });
  }

  uploadedAt(image: ImageEntry): string {
    return this.dates.format(image.uploaded_at);
  }

  /** When a trashed image was taken out of the library. */
  trashedAt(image: ImageEntry): string {
    return image.deleted_at ? this.dates.format(image.deleted_at) : '';
  }

  /** Start editing a name: the id is the only thing the input needs to know. */
  startRename(image: ImageEntry) {
    this.error.set(null);
    this.draftName.set(image.original_filename);
    this.renaming.set(image.id);
  }

  cancelRename() {
    this.renaming.set(null);
    this.draftName.set('');
  }

  /**
   * Replace what an image shows, keeping its id.
   *
   * Nothing about the image's identity changes - not its id, not its name - so every reference to
   * it keeps working while the picture is swapped.
   */
  onReplacementSelected(image: ImageEntry, event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) {
      return;
    }
    this.error.set(null);
    this.notice.set(null);
    this.uploading.set(true);
    this.images.replaceImage(image.id, file).subscribe({
      next: () => {
        this.uploading.set(false);
        input.value = '';
        this.notice.set(t('content.imageReplaced'));
        this.load();
      },
      error: (e) => {
        this.uploading.set(false);
        input.value = '';
        this.error.set(failure('content.replaceFailed', e));
      },
    });
  }

  /**
   * Copy the link that survives a replacement.
   *
   * Written into a Markdown body, this keeps working after the image is replaced; a link to the
   * file itself would point at bytes that are gone.
   */
  async copyLink(image: ImageEntry) {
    this.error.set(null);
    const url = absoluteApiUrl(this.images.imageLink(image.id));
    const copied = await copyToClipboard(url);
    this.notice.set(
      copied ? t('content.imageLinkCopied', { url }) : t('content.imageLinkNotCopied', { url }),
    );
  }

  /**
   * Save the typed name.
   *
   * Checked here rather than left to the server: the name is a label, so the two things that
   * make it unusable - nothing at all, or a path - are worth saying before a round trip.
   */
  confirmRename(image: ImageEntry) {
    const name = this.draftName().trim();
    if (!name || name.includes('/') || name.includes('\\')) {
      this.error.set(t('content.invalidImageName'));
      return;
    }
    if (name === image.original_filename) {
      this.cancelRename();
      return;
    }
    this.images.renameImage(image.id, name).subscribe({
      next: () => {
        this.error.set(null);
        this.cancelRename();
        this.load();
      },
      error: (e) => this.error.set(failure('content.renameFailed', e)),
    });
  }

  /**
   * Take an image out of the library.
   *
   * The undoable half of deleting: the bytes stay, and content that references the image keeps
   * resolving, so this is the one to reach for when something may still be using it.
   */
  trash(image: ImageEntry) {
    this.confirmRemoval(
      image,
      'content.moveToTrashConfirm',
      'content.usedByTrashConfirm',
      (id) => this.images.trashImage(id),
      'content.trashFailed',
    );
  }

  /** Put a trashed image back in the library. */
  restore(image: ImageEntry) {
    this.images.restoreImage(image.id).subscribe({
      next: () => {
        this.error.set(null);
        this.load();
      },
      error: (e) => this.error.set(failure('content.restoreFailed', e)),
    });
  }

  /**
   * Delete an image and its bytes for good, which is the half that cannot be undone.
   *
   * Only offered for an image already in the trash, so it takes two deliberate acts to lose one.
   */
  delete(image: ImageEntry) {
    this.confirmRemoval(
      image,
      'content.deleteImageConfirm',
      'content.usedByDeleteConfirm',
      (id) => this.images.deleteImage(id),
      'content.deleteFailed',
    );
  }

  /**
   * Ask before an image leaves the library, naming what uses it.
   *
   * The list comes from the server's reference index, so the question says what the answer would
   * affect instead of asking the reader to remember: an image that something still shows is worth
   * a different sentence from one that nothing does.
   */
  private confirmRemoval(
    image: ImageEntry,
    quietKey: string,
    usedKey: string,
    act: (id: number) => Observable<unknown>,
    failureKey: string,
  ) {
    this.images.references(image.id).subscribe({
      next: (owners) => {
        const message = owners.length
          ? this.i18n.translate(usedKey, {
              name: image.original_filename,
              count: owners.length,
              list: owners.map((owner) => this.ownerLabel(owner)).join(', '),
            })
          : this.i18n.translate(quietKey, { name: image.original_filename });
        if (!confirm(message)) {
          return;
        }
        act(image.id).subscribe({
          next: () => {
            this.error.set(null);
            this.load();
          },
          error: (e) => this.error.set(failure(failureKey, e)),
        });
      },
      error: (e) => this.error.set(failure('content.failedToLoadReferences', e)),
    });
  }

  /** How one piece of content is named in a warning: a page by name, an item by collection and id. */
  ownerLabel(owner: ImageOwner): string {
    return owner.kind === 'single_page' ? owner.name : `${owner.name} #${owner.item}`;
  }
}
