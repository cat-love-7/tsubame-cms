import { HttpClient } from '@angular/common/http';
import { apiUrl } from 'app/core/api-url';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { ItemMetadata } from 'app/models/item-status';
import { PreviewLink } from 'app/models/links';
import { RelationReference } from 'app/models/relations';

import { CollectionSchema } from 'app/models/schema/collection';
import { SchemaSettings } from 'app/models/schema/settings';
import { FieldValue } from 'app/models/values/fields';
import { ContentValue } from 'app/models/values/single-page';

/**
 * Single pages ("single documents"): one schema and exactly one item per page, unlike a
 * collection which holds many items.
 */
@Injectable({
  providedIn: 'root',
})
export class SinglePageRepository {
  private http = inject(HttpClient);

  /**
   * The title of every page that has one, keyed by page name.
   *
   * A page is named by its name already, so this is only what a page whose schema names a title
   * field says instead of it.
   */
  getPageTitles(): Observable<Record<string, FieldValue>> {
    return this.http.get<Record<string, FieldValue>>(apiUrl('/models/single_pages/titles'));
  }

  listPageNames(): Observable<string[]> {
    return this.http.get<string[]>(apiUrl('/models/single_pages'));
  }

  getPageSchema(name: string): Observable<CollectionSchema> {
    return this.http.get<CollectionSchema>(apiUrl(`/models/single_pages/${name}/schema`));
  }
  /** What the page is told about itself; the defaults for a page that was never given any. */
  getPageSettings(name: string): Observable<SchemaSettings> {
    return this.http.get<SchemaSettings>(apiUrl(`/models/single_pages/${name}/settings`));
  }
  updatePageSettings(name: string, settings: SchemaSettings): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/single_pages/${name}/settings`), settings);
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
  /** The content that points at this page, by the relation index. */
  pageReferences(name: string): Observable<RelationReference[]> {
    return this.http.get<RelationReference[]>(apiUrl(`/models/single_pages/${name}/references`));
  }

  createPreviewLink(name: string): Observable<PreviewLink> {
    return this.http.post<PreviewLink>(apiUrl(`/models/single_pages/${name}/preview-link`), null);
  }
}
