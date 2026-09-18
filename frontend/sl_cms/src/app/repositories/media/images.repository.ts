import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { apiUrl } from 'app/core/api-url';

export interface NewImageRequest {
  original_filename: string;
  ext: string;
}

export interface NewImageInfo {
  id: number;
  /** Backend-relative on-premises, or an absolute presigned URL on AWS. */
  upload_url: string;
  /** Where the image is readable from once uploaded: what content stores. */
  url: string;
}

/** Where to put replacement bytes, and what they will be called once they are there. */
export interface ReplacementInfo {
  file_name: string;
  upload_url: string;
}

/**
 * The content that uses an image, as the server's reference index records it.
 *
 * A collection item carries its id; a single page has exactly one item, so its name is the whole
 * address.
 */
export interface ImageOwner {
  kind: 'collection_item' | 'single_page';
  name: string;
  item?: number | null;
}

/** One image in the library, as the admin screens list it. */
export interface ImageEntry {
  id: number;
  /** Backend-relative (`/images/...`); prefix it with `apiUrl` to load the bytes. */
  url: string;
  original_filename: string;
  uploaded_at: string;
  /** When it was moved to the trash; absent while it is in the library. */
  deleted_at?: string | null;
}

@Injectable({
  providedIn: 'root',
})
export class ImageRepository {
  private http = inject(HttpClient);

  /** Ask the server where to upload an image, and under which id it will be recorded. */
  requestUploadUrl(request: NewImageRequest): Observable<NewImageInfo> {
    return this.http.post<NewImageInfo>(apiUrl('/models/images/get_upload_url'), request);
  }

  /** Upload the bytes to the URL the server handed out. */
  upload(uploadUrl: string, file: Blob): Observable<void> {
    return this.http.put<void>(apiUrl(uploadUrl), file);
  }

  /** Everything that has been uploaded, newest first. */
  listImages(): Observable<ImageEntry[]> {
    return this.http.get<ImageEntry[]>(apiUrl('/models/images'));
  }

  /**
   * Give an image another display name.
   *
   * The id, the bytes and the URL are untouched: the name is a label the library shows, so
   * content that references the image keeps working and a rename can be undone.
   */
  renameImage(id: number, originalFilename: string): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/images/${id}`), {
      original_filename: originalFilename,
    });
  }

  /**
   * Ask where to upload bytes that will replace what an image shows.
   *
   * The image keeps its id and its name; the record is not touched until the bytes are in place,
   * so an upload that fails changes nothing (see {@link applyReplacement}).
   */
  requestReplacement(id: number, ext: string): Observable<ReplacementInfo> {
    return this.http.post<ReplacementInfo>(apiUrl(`/models/images/${id}/replace`), { ext });
  }

  /** Finish a replacement: the record now points at the bytes that were uploaded for it. */
  applyReplacement(id: number, fileName: string): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/images/${id}`), { file_name: fileName });
  }

  /**
   * The durable link to an image, backend-relative.
   *
   * The id never changes, so this keeps working when the bytes are replaced - which is what makes
   * it the right thing to write into a Markdown body. The API answers with a redirect to wherever
   * the image is served from now.
   */
  imageLinkPath(id: number): string {
    return `/images/by-id/${id}`;
  }

  /**
   * Delete an image and its bytes, for good.
   *
   * Only offered for an image already in the trash (see {@link trashImage}), because a reference
   * keeps the id it stored and this is what makes it stop resolving.
   */
  deleteImage(id: number): Observable<void> {
    return this.http.delete<void>(apiUrl(`/models/images/${id}`));
  }

  /** The content that uses an image, so a delete can say what it would break. */
  references(id: number): Observable<ImageOwner[]> {
    return this.http.get<ImageOwner[]>(apiUrl(`/models/images/${id}/references`));
  }

  /** The trash: images taken out of the library, most recently trashed first. */
  listTrash(): Observable<ImageEntry[]> {
    return this.http.get<ImageEntry[]>(apiUrl('/models/images/trash'));
  }

  /**
   * Take an image out of the library, keeping its bytes.
   *
   * Content that references it keeps resolving, so this is the undoable half of deleting: the
   * other half is {@link deleteImage}.
   */
  trashImage(id: number): Observable<void> {
    return this.http.post<void>(apiUrl(`/models/images/${id}/trash`), {});
  }

  /** Put a trashed image back in the library. */
  restoreImage(id: number): Observable<void> {
    return this.http.post<void>(apiUrl(`/models/images/${id}/restore`), {});
  }
}
