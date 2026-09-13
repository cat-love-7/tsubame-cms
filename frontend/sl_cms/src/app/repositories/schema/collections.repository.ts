import { HttpClient } from '@angular/common/http';
import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';
import { CollectionSchema } from 'app/models/schema/collection';
import { ItemMetadata, ItemMetadataMap } from 'app/models/item-status';
import { CollectionItemEntry, CollectionValue } from 'app/models/values/collection';

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
    return this.http.get<string[]>('/api/models/collections');
  }
  getCollectionSchema(name: string): Observable<CollectionSchema> {
    return this.http.get<CollectionSchema>(`/api/models/collections/${name}/schema`);
  }
  updateCollectionSchema(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.put<void>(`/api/models/collections/${name}/schema`, schema);
  }
  createCollection(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.post<void>(`/api/models/collections/${name}/schema`, schema);
  }
  deleteCollection(name: string): Observable<void> {
    return this.http.delete<void>(`/api/models/collections/${name}`);
  }

  // ---- items ---------------------------------------------------------------
  // Item values travel without type tags; the collection schema gives them meaning.

  /** Returns `[id, values]` pairs. */
  listCollectionItems(name: string): Observable<CollectionItemEntry[]> {
    return this.http.get<CollectionItemEntry[]>(`/api/models/collections/${name}/items`);
  }

  getCollectionItem(name: string, id: number): Observable<CollectionValue> {
    return this.http.get<CollectionValue>(`/api/models/collections/${name}/items/${id}`);
  }

  /** Returns the id the server assigned to the new item. */
  createCollectionItem(name: string, values: CollectionValue): Observable<number> {
    return this.http.post<number>(`/api/models/collections/${name}/item`, values);
  }

  updateCollectionItem(name: string, id: number, values: CollectionValue): Observable<void> {
    return this.http.put<void>(`/api/models/collections/${name}/items/${id}`, values);
  }

  deleteCollectionItem(name: string, id: number): Observable<void> {
    return this.http.delete<void>(`/api/models/collections/${name}/items/${id}`);
  }

  // ---- draft / published ---------------------------------------------------
  // Status lives outside the item values, so it never collides with a schema field.

  /** Status of every item, keyed by id. Items that were never published are drafts. */
  listItemMetadata(name: string): Observable<ItemMetadataMap> {
    return this.http.get<ItemMetadataMap>(`/api/models/collections/${name}/items/metadata`);
  }

  getItemMetadata(name: string, id: number): Observable<ItemMetadata> {
    return this.http.get<ItemMetadata>(`/api/models/collections/${name}/items/${id}/metadata`);
  }

  /** Makes the item visible in the public content API. */
  publishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(`/api/models/collections/${name}/items/${id}/publish`, null);
  }

  unpublishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(
      `/api/models/collections/${name}/items/${id}/unpublish`,
      null,
    );
  }
}
