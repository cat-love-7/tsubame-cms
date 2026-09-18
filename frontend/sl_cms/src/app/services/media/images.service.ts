import { Injectable, inject } from '@angular/core';
import { map, Observable, switchMap } from 'rxjs';

import {
  ImageEntry,
  ImageOwner,
  ImageRepository,
  NewImageInfo,
  ReplacementInfo,
} from 'app/repositories/media/images.repository';

@Injectable({
  providedIn: 'root',
})
export class ImagesService {
  private images = inject(ImageRepository);

  /**
   * Upload `file` and resolve to the info the server recorded for it, so the caller can
   * store the image id in the field being edited.
   *
   * The two steps are: ask for a place to put it, then put it there.
   */
  uploadImage(file: File): Observable<NewImageInfo> {
    const extension = file.name.includes('.') ? (file.name.split('.').pop() ?? '') : '';
    return this.images
      .requestUploadUrl({ original_filename: file.name, ext: extension })
      .pipe(switchMap((info) => this.images.upload(info.upload_url, file).pipe(map(() => info))));
  }

  /**
   * Replace what an image shows, keeping its id.
   *
   * Upload first, then point the record at it: the bytes go to a fresh file name (so nothing
   * cached under the old URL can show the old picture), and the image serves what it did until
   * the new bytes are in place.
   */
  replaceImage(id: number, file: File): Observable<ReplacementInfo> {
    const extension = file.name.includes('.') ? (file.name.split('.').pop() ?? '') : '';
    return this.images
      .requestReplacement(id, extension)
      .pipe(
        switchMap((info) =>
          this.images
            .upload(info.upload_url, file)
            .pipe(
              switchMap(() =>
                this.images.applyReplacement(id, info.file_name).pipe(map(() => info)),
              ),
            ),
        ),
      );
  }

  /** The durable link to an image: the id, which survives a replacement. */
  imageLink(id: number): string {
    return this.images.imageLinkPath(id);
  }

  /** The image library, newest first. */
  listImages(): Observable<ImageEntry[]> {
    return this.images.listImages();
  }

  /** Rename an image in the library: the label, not the file behind it. */
  renameImage(id: number, originalFilename: string): Observable<void> {
    return this.images.renameImage(id, originalFilename);
  }

  /** Delete an image and its bytes, for good. */
  deleteImage(id: number): Observable<void> {
    return this.images.deleteImage(id);
  }

  /** The content that uses an image, so a delete can say what it would break. */
  references(id: number): Observable<ImageOwner[]> {
    return this.images.references(id);
  }

  /** The trash: images taken out of the library, most recently trashed first. */
  listTrash(): Observable<ImageEntry[]> {
    return this.images.listTrash();
  }

  /** Take an image out of the library, keeping its bytes and its references working. */
  trashImage(id: number): Observable<void> {
    return this.images.trashImage(id);
  }

  /** Put a trashed image back in the library. */
  restoreImage(id: number): Observable<void> {
    return this.images.restoreImage(id);
  }
}
