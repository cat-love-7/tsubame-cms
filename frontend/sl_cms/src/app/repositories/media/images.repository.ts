import { HttpClient } from '@angular/common/http';
import { Injectable } from '@angular/core';
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

/** One image in the library, as the admin screens list it. */
export interface ImageEntry {
  id: number;
  /** Backend-relative (`/images/...`); prefix it with `apiUrl` to load the bytes. */
  url: string;
  original_filename: string;
  uploaded_at: string;
}

@Injectable({
  providedIn: 'root',
})
export class ImageRepository {
  constructor(private http: HttpClient) {}

  /** Ask the server where to upload an image, and under which id it will be recorded. */
  requestUploadUrl(request: NewImageRequest): Observable<NewImageInfo> {
    return this.http.post<NewImageInfo>('/api/models/images/get_upload_url', request);
  }

  /** Upload the bytes to the URL the server handed out. */
  upload(uploadUrl: string, file: Blob): Observable<void> {
    return this.http.put<void>(apiUrl(uploadUrl), file);
  }

  /** Everything that has been uploaded, newest first. */
  listImages(): Observable<ImageEntry[]> {
    return this.http.get<ImageEntry[]>('/api/models/images');
  }

  /**
   * Delete an image and its bytes.
   *
   * Nothing checks whether content still references it: a reference keeps the id it stored
   * and simply stops resolving.
   */
  deleteImage(id: number): Observable<void> {
    return this.http.delete<void>(`/api/models/images/${id}`);
  }
}
