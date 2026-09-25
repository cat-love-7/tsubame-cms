import { Injectable, inject } from '@angular/core';
import { Message } from 'app/core/i18n/message';
import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { makeThumbnail, thumbnailExtension } from 'app/core/image-thumbnail';
import { Observable, catchError, from, map, of, switchMap, throwError } from 'rxjs';

import {
  ImageOwner,
  ImagePage,
  ImageRepository,
  NewImageInfo,
  ReplacementInfo,
} from 'app/repositories/media/images.repository';

/** How many tiles one page of the library holds. */
export const IMAGE_PAGE_SIZE = 60;

@Injectable({
  providedIn: 'root',
})
export class ImagesService {
  private images = inject(ImageRepository);
  private capabilities = inject(CapabilitiesService);

  /**
   * Upload `file` and resolve to the info the server recorded for it, so the caller can
   * store the image id in the field being edited.
   *
   * The two steps are: ask for a place to put it, then put it there.
   */
  uploadImage(file: File): Observable<NewImageInfo> {
    const extension = file.name.includes('.') ? (file.name.split('.').pop() ?? '') : '';
    const refusal = this.refusalFor(file);
    if (refusal) {
      return throwError(() => refusal);
    }
    return this.images
      .requestUploadUrl({ original_filename: file.name, ext: extension, size: file.size })
      .pipe(
        switchMap((info) =>
          this.images
            .upload(info.upload_url, file)
            .pipe(switchMap(() => this.sendThumbnail(info.id, file).pipe(map(() => info)))),
        ),
      );
  }

  /**
   * Make the small copy of `file` and store it against `id`.
   *
   * Deliberately best-effort: the picture is uploaded and usable either way, and only the tiles
   * are heavier without it. A browser that cannot decode the file (`makeThumbnail` answers `null`),
   * or a server that refuses the copy, must not turn a successful upload into a failure an editor
   * sees - so every failure here becomes "no small copy".
   */
  private sendThumbnail(id: number, file: File): Observable<void> {
    return from(makeThumbnail(file)).pipe(
      switchMap((thumbnail) =>
        thumbnail
          ? this.images.putThumbnail(id, thumbnailExtension(thumbnail), thumbnail)
          : of(undefined),
      ),
      catchError(() => of(undefined)),
    );
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
    const refusal = this.refusalFor(file);
    if (refusal) {
      return throwError(() => refusal);
    }
    return this.images
      .requestReplacement(id, extension, file.size)
      .pipe(
        switchMap((info) =>
          this.images
            .upload(info.upload_url, file)
            .pipe(
              switchMap(() =>
                this.images
                  .applyReplacement(id, info.file_name)
                  .pipe(switchMap(() => this.sendThumbnail(id, file).pipe(map(() => info)))),
              ),
            ),
        ),
      );
  }

  /**
   * Refuse a file the deployment will not take, before asking where to put it.
   *
   * The browser knows the size and the deployment has said what its limit is (`GET
   * /auth/capabilities`), so sending megabytes only to be refused is a waste of the reader's
   * connection. With no limit known - an older server, or an answer that has not arrived - the
   * upload goes ahead and the server's own refusal is what the reader sees.
   */
  private refusalFor(file: File): Message | null {
    const limit = this.capabilities.maxImageBytes();
    if (limit === null || file.size <= limit) {
      return null;
    }
    return {
      key: 'content.imageTooLarge',
      params: { size: megabytes(file.size), max: megabytes(limit) },
    };
  }

  /** The durable link to an image: the id, which survives a replacement. */
  imageLink(id: number): string {
    return this.images.imageLinkPath(id);
  }

  /** One page of the image library, newest first. */
  listImages(offset = 0, limit = IMAGE_PAGE_SIZE): Observable<ImagePage> {
    return this.images.listImages({ limit, offset });
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

  /** One page of the trash, most recently trashed first. */
  listTrash(offset = 0, limit = IMAGE_PAGE_SIZE): Observable<ImagePage> {
    return this.images.listTrash({ limit, offset });
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

/** A byte count as a reader reads it: megabytes, to one decimal. */
function megabytes(bytes: number): number {
  return Math.round((bytes / (1024 * 1024)) * 10) / 10;
}
