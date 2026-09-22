import { HttpClient, HttpResponse } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { map, Observable } from 'rxjs';

import { apiUrl } from 'app/core/api-url';

export interface NewImageRequest {
  original_filename: string;
  ext: string;
  /**
   * How many bytes will be uploaded.
   *
   * The server needs it before the bytes: it refuses an image over the deployment's limit, and on
   * a deployment that signs the upload it is part of the signature, so storage takes only an
   * upload of that size.
   */
  size: number;
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
  /**
   * The small copy to show in a tile, when one has been stored; absent for an image uploaded
   * through the API rather than a browser, which the screens then show at full size.
   */
  thumbnail_url?: string | null;
  original_filename: string;
  uploaded_at: string;
  /** When it was moved to the trash; absent while it is in the library. */
  deleted_at?: string | null;
}

/** One page of a library list, with how many there are altogether. */
export interface ImagePage {
  images: ImageEntry[];
  /** Images in the whole list, not only in this page. */
  total: number;
}

/** The body the API answers with, plus the total it reports in a header. */
function pageOf(response: HttpResponse<ImageEntry[]>): ImagePage {
  const images = response.body ?? [];
  // An older server does not send the header; the page it answered is then the whole list.
  const total = Number(response.headers.get('x-total-count') ?? images.length);
  return { images, total: Number.isFinite(total) ? total : images.length };
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

  /**
   * One page of what has been uploaded, newest first.
   *
   * The library renders tiles, and a browser that has to lay out thousands of them at once is
   * slower than one that is handed the next page when the reader asks for it. `total` comes from
   * the server so the screen can say how many there are without reading them.
   */
  listImages(page: { limit: number; offset: number }): Observable<ImagePage> {
    return this.http
      .get<ImageEntry[]>(apiUrl('/models/images'), {
        params: { limit: page.limit, offset: page.offset },
        observe: 'response',
      })
      .pipe(map((response) => pageOf(response)));
  }

  /** Store the small copy of an image: what the tiles show instead of the original. */
  putThumbnail(id: number, ext: string, blob: Blob): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/images/${id}/thumbnail`), blob, {
      params: { ext },
    });
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
  requestReplacement(id: number, ext: string, size: number): Observable<ReplacementInfo> {
    return this.http.post<ReplacementInfo>(apiUrl(`/models/images/${id}/replace`), { ext, size });
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

  /** One page of the trash, most recently trashed first. */
  listTrash(page: { limit: number; offset: number }): Observable<ImagePage> {
    return this.http
      .get<ImageEntry[]>(apiUrl('/models/images/trash'), {
        params: { limit: page.limit, offset: page.offset },
        observe: 'response',
      })
      .pipe(map((response) => pageOf(response)));
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
