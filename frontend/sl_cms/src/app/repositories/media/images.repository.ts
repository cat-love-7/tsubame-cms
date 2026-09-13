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
}
