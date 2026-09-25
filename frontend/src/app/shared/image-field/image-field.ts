import { Component, EventEmitter, Input, Output, inject, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { TranslocoPipe } from '@jsverse/transloco';

import { apiUrl } from 'app/core/api-url';
import { Message, failure } from 'app/core/i18n/message';
import { FieldValue, imageIdOf } from 'app/models/values/fields';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { ImagesService } from 'app/services/media/images.service';
import { LibraryPicker } from 'app/shared/library-picker/library-picker';

/**
 * A single image value: the picture, its id, and the two ways to set it.
 *
 * An editor either uploads a file or picks one already in the library; both end in the same value
 * shape (`{id, url}`), so nothing downstream has to know which was used. The **stable** url comes
 * from the server - on AWS an upload url names a signature, not the object's address, so it cannot
 * be derived here.
 */
@Component({
  selector: 'app-image-field',
  imports: [LibraryPicker, MatButtonModule, MatIconModule, TranslocoPipe],
  templateUrl: './image-field.html',
  styleUrl: './image-field.scss',
})
export class ImageField {
  private images = inject(ImagesService);

  @Input() value: FieldValue = null;
  /** Renders the buttons away, for the schema editor's preview. */
  @Input() disabled = false;
  @Input() labelId = '';
  @Output() valueChange = new EventEmitter<FieldValue>();
  @Output() errorChange = new EventEmitter<Message | null>();

  public uploading = signal(false);
  public pickerOpen = signal(false);
  public imageUrl = apiUrl;

  imageId(): number | null {
    return imageIdOf(this.value);
  }

  imagePreviewUrl(): string {
    const value = this.value;
    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
      const url = (value as { url?: unknown }).url;
      if (typeof url === 'string') {
        return url;
      }
    }
    return '';
  }

  onFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) {
      return;
    }
    this.uploading.set(true);
    this.errorChange.emit(null);
    this.images.uploadImage(file).subscribe({
      next: (info) => {
        this.uploading.set(false);
        // Mirror the shape the API returns for an image value.
        this.valueChange.emit({ id: info.id, url: info.url });
        input.value = '';
      },
      error: (e) => {
        this.uploading.set(false);
        this.errorChange.emit(failure('content.uploadFailed', e));
      },
    });
  }

  openLibrary() {
    this.pickerOpen.set(true);
  }

  closePicker() {
    this.pickerOpen.set(false);
  }

  /** Use a library image; the value keeps the same shape an upload produces. */
  chooseImage(image: ImageEntry) {
    this.valueChange.emit({ id: image.id, url: image.url });
    this.closePicker();
    this.errorChange.emit(null);
  }
}
