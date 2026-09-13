import { Injectable } from '@angular/core';
import { map, Observable, switchMap } from 'rxjs';

import {
  ImageEntry,
  ImageRepository,
  NewImageInfo,
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

  /** The image library, newest first. */
  listImages(): Observable<ImageEntry[]> {
    return this.images.listImages();
  }

  deleteImage(id: number): Observable<void> {
    return this.images.deleteImage(id);
  }
}
