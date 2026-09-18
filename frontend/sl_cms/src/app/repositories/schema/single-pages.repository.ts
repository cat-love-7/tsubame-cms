import { HttpClient } from '@angular/common/http';
import { apiUrl } from 'app/core/api-url';
import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';

import { ItemMetadata } from 'app/models/item-status';
import { PreviewLink } from 'app/models/links';

import { CollectionSchema } from 'app/models/schema/collection';
import { ContentValue } from 'app/models/values/single-page';

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
    return this.http.get<string[]>(apiUrl('/models/single_pages'));
  }

  getPageSchema(name: string): Observable<CollectionSchema> {
    return this.http.get<CollectionSchema>(apiUrl(`/models/single_pages/${name}/schema`));
  }

  createPage(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.post<void>(apiUrl(`/models/single_pages/${name}/schema`), schema);
  }

  updatePageSchema(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/single_pages/${name}/schema`), schema);
  }

  deletePage(name: string): Observable<void> {
    return this.http.delete<void>(apiUrl(`/models/single_pages/${name}`));
  }

  /** The page's content. The server answers with defaults for anything never set. */
  getPageItem(name: string): Observable<ContentValue> {
    return this.http.get<ContentValue>(apiUrl(`/models/single_pages/${name}/item`));
  }

  updatePageItem(name: string, values: ContentValue): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/single_pages/${name}/item`), values);
  }

  // ---- draft / published ---------------------------------------------------

  getPageMetadata(name: string): Observable<ItemMetadata> {
    return this.http.get<ItemMetadata>(apiUrl(`/models/single_pages/${name}/item/metadata`));
  }

  /**
   * The state of every page, keyed by name, in one request.
   *
   * The list shows a status per page; asking one by one would be a request each.
   */
  listItemMetadata(): Observable<{ [name: string]: ItemMetadata }> {
    return this.http.get<{ [name: string]: ItemMetadata }>(
      apiUrl('/models/single_pages/items/metadata'),
    );
  }

  publishPage(name: string): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(apiUrl(`/models/single_pages/${name}/publish`), null);
  }

  unpublishPage(name: string): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(apiUrl(`/models/single_pages/${name}/unpublish`), null);
  }

  /** Mints a link that shows this working copy to someone without an account. */
  createPreviewLink(name: string): Observable<PreviewLink> {
    return this.http.post<PreviewLink>(apiUrl(`/models/single_pages/${name}/preview-link`), null);
  }
}
