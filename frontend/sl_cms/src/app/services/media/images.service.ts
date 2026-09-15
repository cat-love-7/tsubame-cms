import { Injectable } from '@angular/core';
import { map, Observable, switchMap } from 'rxjs';

import {
  ImageEntry,
  ImageRepository,
  NewImageInfo,
  ReplacementInfo,
} from 'app/repositories/media/images.repository';

@Injectable({
  providedIn: 'root',
})
export class ImagesService {
  constructor(private images: ImageRepository) {}

  /**
   * Upload `file` and resolve to the info the server recorded for it, so the caller can
   * store the image id in the field being edited.
   *
   * The two steps are: ask for a place to put it, then put it there.
   */
  uploadImage(file: File): Observable<NewImageInfo> {
    const extension = file.name.includes('.') ? file.name.split('.').pop() ?? '' : '';
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
    const extension = file.name.includes('.') ? file.name.split('.').pop() ?? '' : '';
    return this.images
      .requestReplacement(id, extension)
      .pipe(
        switchMap((info) =>
          this.images
            .upload(info.upload_url, file)
            .pipe(switchMap(() => this.images.applyReplacement(id, info.file_name).pipe(map(() => info)))),
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

  deleteImage(id: number): Observable<void> {
    return this.images.deleteImage(id);
  }
}
