import { HttpClient } from '@angular/common/http';
import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';

import { ItemMetadata } from 'app/models/item-status';
import { CollectionSchema } from 'app/models/schema/collection';
import { ContentValue } from 'app/models/values/collection';

/**
 * Single pages ("single documents"): one schema and exactly one item per page, unlike a
 * collection which holds many items.
 */
@Injectable({
  providedIn: 'root',
})
export class SinglePageRepository {
  constructor(private http: HttpClient) {}

  listPageNames(): Observable<string[]> {
    return this.http.get<string[]>('/api/models/single_pages');
  }

  getPageSchema(name: string): Observable<CollectionSchema> {
    return this.http.get<CollectionSchema>(`/api/models/single_pages/${name}/schema`);
  }

  createPage(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.post<void>(`/api/models/single_pages/${name}/schema`, schema);
  }

  updatePageSchema(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.put<void>(`/api/models/single_pages/${name}/schema`, schema);
  }

  deletePage(name: string): Observable<void> {
    return this.http.delete<void>(`/api/models/single_pages/${name}`);
  }

  /** The page's content. The server answers with defaults for anything never set. */
  getPageItem(name: string): Observable<ContentValue> {
    return this.http.get<ContentValue>(`/api/models/single_pages/${name}/item`);
  }

  updatePageItem(name: string, values: ContentValue): Observable<void> {
    return this.http.put<void>(`/api/models/single_pages/${name}/item`, values);
  }

  // ---- draft / published ---------------------------------------------------

  getPageMetadata(name: string): Observable<ItemMetadata> {
    return this.http.get<ItemMetadata>(`/api/models/single_pages/${name}/item/metadata`);
  }

  publishPage(name: string): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(`/api/models/single_pages/${name}/publish`, null);
  }

  unpublishPage(name: string): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(`/api/models/single_pages/${name}/unpublish`, null);
  }
}
