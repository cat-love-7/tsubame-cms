import { HttpClient } from '@angular/common/http';
import { apiUrl } from 'app/core/api-url';
import { Injectable } from '@angular/core';
import { map, Observable } from 'rxjs';
import { CollectionSchema } from 'app/models/schema/collection';
import { ItemMetadata, ItemMetadataMap, ItemStatus, ItemStatusOutcome } from 'app/models/item-status';
import { PreviewLink } from 'app/models/links';

import { CollectionItemEntry, CollectionItemPage, CollectionValue } from 'app/models/values/collection';

/**
 * `providedIn: 'root'` so every consumer (and every TestBed) gets the same instance
 * without the per-component provider duplication this used to need.
 */
@Injectable({
  providedIn: 'root',
})
export class CollectionRepository {
  constructor(private http: HttpClient) {}
  getAllCollectionNames(): Observable<string[]> {
    return this.http.get<string[]>(apiUrl('/models/collections'));
  }
  getCollectionSchema(name: string): Observable<CollectionSchema> {
    return this.http.get<CollectionSchema>(apiUrl(`/models/collections/${name}/schema`));
  }
  updateCollectionSchema(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/collections/${name}/schema`), schema);
  }
  createCollection(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.post<void>(apiUrl(`/models/collections/${name}/schema`), schema);
  }
  deleteCollection(name: string): Observable<void> {
    return this.http.delete<void>(apiUrl(`/models/collections/${name}`));
  }

  // ---- items ---------------------------------------------------------------
  // Item values travel without type tags; the collection schema gives them meaning.

  /**
   * One page of items.
   *
   * The total is not part of the body — it comes back in `X-Total-Count` — so the whole
   * response is read here and reduced to a page the caller can use directly.
   */
  listCollectionItemsPage(name: string, limit: number, offset: number): Observable<CollectionItemPage> {
    return this.http
      .get<CollectionItemEntry[]>(apiUrl(`/models/collections/${name}/items`), {
        params: { limit, offset },
        observe: 'response',
      })
      .pipe(
        map((response) => ({
          items: response.body ?? [],
          total: Number(response.headers.get('X-Total-Count') ?? 0),
        })),
      );
  }

  getCollectionItem(name: string, id: number): Observable<CollectionValue> {
    return this.http.get<CollectionValue>(apiUrl(`/models/collections/${name}/items/${id}`));
  }

  /** Returns the id the server assigned to the new item. */
  createCollectionItem(name: string, values: CollectionValue): Observable<number> {
    return this.http.post<number>(apiUrl(`/models/collections/${name}/item`), values);
  }

  updateCollectionItem(name: string, id: number, values: CollectionValue): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/collections/${name}/items/${id}`), values);
  }

  deleteCollectionItem(name: string, id: number): Observable<void> {
    return this.http.delete<void>(apiUrl(`/models/collections/${name}/items/${id}`));
  }

  // ---- draft / published ---------------------------------------------------
  // Status lives outside the item values, so it never collides with a schema field.

  /** Status of every item, keyed by id. Items that were never published are drafts. */
  listItemMetadata(name: string): Observable<ItemMetadataMap> {
    return this.http.get<ItemMetadataMap>(apiUrl(`/models/collections/${name}/items/metadata`));
  }

  getItemMetadata(name: string, id: number): Observable<ItemMetadata> {
    return this.http.get<ItemMetadata>(apiUrl(`/models/collections/${name}/items/${id}/metadata`));
  }

  /** Makes the item visible in the public content API. */
  /** Publish or unpublish a batch, answering per item (see `ItemStatusOutcome` on the server). */
  setItemsStatus(name: string, ids: number[], status: ItemStatus): Observable<ItemStatusOutcome[]> {
    return this.http.post<ItemStatusOutcome[]>(apiUrl(`/models/collections/${name}/items/status`), {
      ids,
      status,
    });
  }

  /** Copy an item: the server answers with the new item's id. */
  duplicateItem(name: string, id: number): Observable<number> {
    return this.http.post<number>(apiUrl(`/models/collections/${name}/items/${id}/duplicate`), null);
  }

  publishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(apiUrl(`/models/collections/${name}/items/${id}/publish`), null);
  }

  unpublishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(
      apiUrl(`/models/collections/${name}/items/${id}/unpublish`),
      null,
    );
  }

  /** Mints a link that shows this working copy to someone without an account. */
  createPreviewLink(name: string, id: number): Observable<PreviewLink> {
    return this.http.post<PreviewLink>(
      apiUrl(`/models/collections/${name}/items/${id}/preview-link`),
      null,
    );
  }
}
