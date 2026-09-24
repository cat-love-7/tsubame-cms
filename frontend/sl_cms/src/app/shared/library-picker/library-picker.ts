import {
  Component,
  EventEmitter,
  Input,
  OnChanges,
  Output,
  SimpleChanges,
  computed,
  inject,
  signal,
} from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { TranslocoPipe } from '@jsverse/transloco';

import { apiUrl } from 'app/core/api-url';
import { Message, failure } from 'app/core/i18n/message';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { ImagesService } from 'app/services/media/images.service';

/**
 * The image library, as a picker: a wall of tiles to choose one image from, or several.
 *
 * Used by the image field, the image array and the Markdown box's image button - which is why it
 * is a component of its own rather than part of the widget that happened to need it first. It owns
 * what is only about picking (which page of the library is loaded, which tiles are ticked) and
 * says what was chosen; the field decides what a chosen image means.
 */
@Component({
  selector: 'app-library-picker',
  imports: [MatButtonModule, MatIconModule, TranslocoPipe],
  templateUrl: './library-picker.html',
  styleUrl: './library-picker.scss',
})
export class LibraryPicker implements OnChanges {
  private images = inject(ImagesService);

  /** Whether it is on screen. The field says when to open it; the picker says when to close. */
  @Input() open = false;
  /** True when it collects several images at once (an image array). */
  @Input() multi = false;

  /** One image, chosen. */
  @Output() chosen = new EventEmitter<ImageEntry>();
  /** Several images, ticked and confirmed, in the order the library lists them. */
  @Output() chosenMany = new EventEmitter<ImageEntry[]>();
  /** The reader is done: close the picker. */
  @Output() closed = new EventEmitter<void>();
  /** The library could not be read, for the field to report. */
  @Output() failed = new EventEmitter<Message>();

  public library = signal<ImageEntry[]>([]);
  /** How many the library holds altogether, which is more than one page shows. */
  public libraryTotal = signal(0);
  /** Whether the next page is on its way. */
  public loadingMore = signal(false);
  /** Ids ticked in the multi-image mode. */
  public selected = signal<number[]>([]);
  /** The library is fetched when the picker is first opened, and not before. */
  private loaded = false;
  /** Exposed for the template. */
  public imageUrl = apiUrl;

  ngOnChanges(changes: SimpleChanges) {
    if (changes['open']?.currentValue !== true) {
      return;
    }
    // Opening starts from nothing ticked: the ticks of the last pick are not a choice about this
    // one.
    this.selected.set([]);
    this.load();
  }

  /** Fetch the first page of the library, once: a second open reuses what was already read. */
  private load() {
    if (this.loaded) {
      return;
    }
    this.images.listImages().subscribe({
      next: (page) => {
        this.loaded = true;
        this.library.set(page.images);
        this.libraryTotal.set(page.total);
      },
      error: (e) => this.failed.emit(failure('content.failedToLoadImages', e)),
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
        this.failed.emit(failure('content.failedToLoadImages', e));
      },
    });
  }

  close() {
    this.closed.emit();
  }

  /** A thumbnail click: one image uses it, several ticks it. */
  onThumbnail(image: ImageEntry) {
    if (!this.multi) {
      this.chosen.emit(image);
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
    this.chosenMany.emit(chosen);
  }
}
