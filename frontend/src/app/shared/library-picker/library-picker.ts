import { Component, computed, inject, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import {
  MAT_DIALOG_DATA,
  MatDialog,
  MatDialogModule,
  MatDialogRef,
} from '@angular/material/dialog';
import { MatIconModule } from '@angular/material/icon';
import { TranslocoPipe } from '@jsverse/transloco';
import { Observable } from 'rxjs';

import { apiUrl } from 'app/core/api-url';
import { Message, MessagePipe, failure } from 'app/core/i18n/message';
import { ImageEntry, NewImageInfo } from 'app/repositories/media/images.repository';
import { ImagesService } from 'app/services/media/images.service';

/** What the picker is opened with. */
export interface LibraryPickerData {
  /** True when it collects several images at once (an image array). */
  multi: boolean;
}

/** What a pick closes with: one image, several, or nothing when it was cancelled. */
export type LibraryPickerResult = ImageEntry | ImageEntry[] | undefined;

/**
 * The image library, as a picker: a wall of tiles **in a dialog**, to choose one image from or
 * several, and to upload one that is not in the library yet.
 *
 * Used by the image field, the image array and the Markdown box's image button - which is why it is
 * a component of its own rather than part of the widget that happened to need it first. It owns what
 * is only about picking (which page of the library is loaded, which tiles are ticked, what is being
 * uploaded) and answers with what was chosen; the field decides what a chosen image means.
 *
 * A dialog rather than a panel under the control that opened it: a wall of tiles is taller than the
 * form around it, so in place it pushed everything below it down and then had to be scrolled inside
 * a 200px box; and the Markdown box had no upload of its own at all, so an empty library was a dead
 * end there. The dialog carries the upload, and closes as soon as an image is chosen.
 */
@Component({
  selector: 'app-library-picker',
  imports: [MatButtonModule, MatDialogModule, MatIconModule, MessagePipe, TranslocoPipe],
  templateUrl: './library-picker.html',
  styleUrl: './library-picker.scss',
})
export class LibraryPicker {
  private images = inject(ImagesService);
  private dialog = inject<MatDialogRef<LibraryPicker, LibraryPickerResult>>(MatDialogRef);

  /** Whether it is collecting one image or several. */
  public data = inject<LibraryPickerData>(MAT_DIALOG_DATA);

  public library = signal<ImageEntry[]>([]);
  /** How many the library holds altogether, which is more than one page shows. */
  public libraryTotal = signal(0);
  /** Whether the next page is on its way. */
  public loadingMore = signal(false);
  /** Whether a file is on its way up. */
  public uploading = signal(false);
  /** Ids ticked in the multi-image mode. */
  public selected = signal<number[]>([]);
  /**
   * What went wrong - the library could not be read, or an upload failed.
   *
   * Shown *in* the dialog rather than handed back to the field: the field is behind a modal, so a
   * message left there is one nobody sees.
   */
  public error = signal<Message | null>(null);
  /** Exposed for the template. */
  public imageUrl = apiUrl;

  constructor() {
    // Read per opening: there is nothing to reuse between two openings, and a fresh read is what
    // shows an image somebody else uploaded a moment ago.
    this.load();
  }

  /** Fetch the first page of the library. */
  private load() {
    this.images.listImages().subscribe({
      next: (page) => {
        this.library.set(page.images);
        this.libraryTotal.set(page.total);
      },
      error: (e) => this.error.set(failure('content.failedToLoadImages', e)),
    });
  }

  /** Whether the library holds images this picker has not been handed yet. */
  public canLoadMore = computed(() => this.library().length < this.libraryTotal());

  /** Ask for the next page, without closing the picker. */
  loadMore() {
    if (this.loadingMore() || !this.canLoadMore()) {
      return;
    }
    this.loadingMore.set(true);
    this.images.listImages(this.library().length).subscribe({
      next: (page) => {
        this.loadingMore.set(false);
        this.library.set([...this.library(), ...page.images]);
        this.libraryTotal.set(page.total);
      },
      error: (e) => {
        this.loadingMore.set(false);
        this.error.set(failure('content.failedToLoadImages', e));
      },
    });
  }

  /** Close without choosing anything. */
  close() {
    this.dialog.close(undefined);
  }

  /** A thumbnail click: one image uses it, several ticks it. */
  onThumbnail(image: ImageEntry) {
    if (!this.data.multi) {
      this.dialog.close(image);
      return;
    }
    this.selected.update((ids) =>
      ids.includes(image.id) ? ids.filter((id) => id !== image.id) : [...ids, image.id],
    );
  }

  isSelected(id: number): boolean {
    return this.selected().includes(id);
  }

  /** Confirm the ticked images, in the order the library lists them. */
  addSelected() {
    const chosen = this.library().filter((image) => this.selected().includes(image.id));
    this.dialog.close(chosen);
  }

  /**
   * A file chosen in the dialog: it is uploaded, and then it is the pick.
   *
   * One file at a time, which is what a tile can show and what the library's own upload does: a
   * second file is a second gesture, and that is honest about the wait.
   */
  onFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    // Cleared so that choosing the same file again still fires a change.
    input.value = '';
    if (!file) {
      return;
    }
    this.uploading.set(true);
    this.error.set(null);
    this.images.uploadImage(file).subscribe({
      next: (info) => {
        this.uploading.set(false);
        const uploaded = uploadedEntry(info, file);
        if (!this.data.multi) {
          this.dialog.close(uploaded);
          return;
        }
        // Several at once: the upload joins the wall already ticked, so the pick can carry on.
        this.library.update((images) => [uploaded, ...images]);
        this.libraryTotal.update((total) => total + 1);
        this.selected.update((ids) => [...ids, uploaded.id]);
      },
      error: (e) => {
        this.uploading.set(false);
        this.error.set(failure('content.uploadFailed', e));
      },
    });
  }
}

/**
 * The tile for an image that has just been uploaded.
 *
 * The upload answers where the bytes are, not what the library lists: a tile also needs a name (it
 * is the `title` and the alt text), and the browser is still holding that. The rest of the record -
 * the small copy, the deletion time - is what the library fills in when it is read again.
 */
function uploadedEntry(info: NewImageInfo, file: File): ImageEntry {
  return {
    id: info.id,
    url: info.url,
    original_filename: file.name,
    uploaded_at: new Date().toISOString(),
  };
}

/** Open the picker and answer what was chosen, or `undefined` when it was closed. */
function openPicker(dialog: MatDialog, multi: boolean): Observable<LibraryPickerResult> {
  return dialog
    .open<LibraryPicker, LibraryPickerData, LibraryPickerResult>(LibraryPicker, {
      data: { multi },
      // Wide enough for four tiles, and never wider than the window it is in.
      width: '44rem',
      maxWidth: '92vw',
      // The wall is the point of the dialog, so Tab starts there rather than on the close button.
      autoFocus: 'dialog',
    })
    .afterClosed();
}

/** One image, or `undefined`: the dialog answers one image exactly when `multi` is false. */
export function openImagePicker(dialog: MatDialog): Observable<ImageEntry | undefined> {
  return openPicker(dialog, false) as Observable<ImageEntry | undefined>;
}

/** Several images, or `undefined`: the dialog answers an array exactly when `multi` is true. */
export function openImagePickerMany(dialog: MatDialog): Observable<ImageEntry[] | undefined> {
  return openPicker(dialog, true) as Observable<ImageEntry[] | undefined>;
}
