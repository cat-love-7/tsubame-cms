import { Component, inject, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';

import { AuthService } from 'app/core/auth/auth.service';
import { apiUrl } from 'app/core/api-url';
import { errorMessage as message } from 'app/core/http-error';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { ImagesService } from 'app/services/media/images.service';

/**
 * The image library.
 *
 * Shows everything that has been uploaded, newest first, and does the two things an editor
 * needs from it: add an image, and remove one. The content forms pick from this same list,
 * so an image uploaded once can be reused instead of uploaded again.
 */
@Component({
  selector: 'app-image-library',
  imports: [MatButtonModule, MatIconModule],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class List {
  private images = inject(ImagesService);
  /** Uploading and deleting images is an editing action. */
  public auth = inject(AuthService);

  public library = signal<ImageEntry[]>([]);
  public error = signal('');
  public uploading = signal(false);
  /** Exposed for the template. */
  public imageUrl = apiUrl;

  constructor() {
    this.load();
  }

  private load() {
    this.images.listImages().subscribe({
      next: (images) => this.library.set(images),
      error: (e) => this.error.set(`Failed to load the images: ${message(e)}`),
    });
  }

  onFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) {
      return;
    }
    this.uploading.set(true);
    this.error.set('');
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
        this.error.set(`Upload failed: ${message(e)}`);
      },
    });
  }

  uploadedAt(image: ImageEntry): string {
    return new Date(image.uploaded_at).toLocaleString();
  }

  delete(image: ImageEntry) {
    if (!confirm(`Delete ${image.original_filename}?`)) {
      return;
    }
    this.images.deleteImage(image.id).subscribe({
      next: () => {
        this.error.set('');
        this.load();
      },
      error: (e) => this.error.set(`Delete failed: ${message(e)}`),
    });
  }
}
