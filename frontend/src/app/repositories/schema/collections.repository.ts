import { HttpClient } from '@angular/common/http';
import { apiUrl } from 'app/core/api-url';
import { Injectable, inject } from '@angular/core';
import { map, Observable } from 'rxjs';
import { CollectionSchema } from 'app/models/schema/collection';
import { SchemaSettings } from 'app/models/schema/settings';
import { FieldValue } from 'app/models/values/fields';
import {
  ItemMetadata,
  ItemMetadataMap,
  ItemStatus,
  ItemStatusOutcome,
} from 'app/models/item-status';
import { PreviewLink } from 'app/models/links';
import { RelationReference } from 'app/models/relations';

import {
  CollectionItemEntry,
  CollectionItemPage,
  CollectionValue,
} from 'app/models/values/collection';

/**
 * `providedIn: 'root'` so every consumer (and every TestBed) gets the same instance
 * without the per-component provider duplication this used to need.
 */
@Injectable({
  providedIn: 'root',
})
export class CollectionRepository {
  private http = inject(HttpClient);
  getAllCollectionNames(): Observable<string[]> {
    return this.http.get<string[]>(apiUrl('/models/collections'));
  }
  getCollectionSchema(name: string): Observable<CollectionSchema> {
    return this.http.get<CollectionSchema>(apiUrl(`/models/collections/${name}/schema`));
  }
  updateCollectionSchema(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/collections/${name}/schema`), schema);
  }
  /**
   * What the collection is told about itself. A collection that was never given any settings
   * answers the defaults, so this is never a 404 for a collection that exists.
   */
  getCollectionSettings(name: string): Observable<SchemaSettings> {
    return this.http.get<SchemaSettings>(apiUrl(`/models/collections/${name}/settings`));
  }
  updateCollectionSettings(name: string, settings: SchemaSettings): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/collections/${name}/settings`), settings);
  }
  /**
   * The titles of the named items: what a reference to one shows as its name.
   *
   * Keyed by id written as a string, and an id the server has no title for is left out.
   */
  getCollectionItemTitles(name: string, ids: number[]): Observable<Record<string, FieldValue>> {
    return this.http.get<Record<string, FieldValue>>(
      apiUrl(`/models/collections/${name}/items/titles`),
      {
        params: { ids: ids.join(',') },
      },
    );
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
  listCollectionItemsPage(
    name: string,
    limit: number,
    offset: number,
    sort?: string,
  ): Observable<CollectionItemPage> {
    return this.http
      .get<CollectionItemEntry[]>(apiUrl(`/models/collections/${name}/items`), {
        params: sort ? { limit, offset, sort } : { limit, offset },
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

  /**
   * What the site serves for this item right now.
   *
   * {@link getCollectionItem} answers with the *working* copy, which is what the editor is holding;
   * this is the other side of it, for an editor deciding what to do with their changes. A 404 means
   * the item is not published, so there is nothing live to compare against.
   */
  getPublishedCollectionItem(name: string, id: number): Observable<CollectionValue> {
    return this.http.get<CollectionValue>(
      apiUrl(`/models/collections/${name}/items/${id}/published`),
    );
  }

  /**
   * Throw the working copy away: the item goes back to what the site is serving.
   *
   * Idempotent, and it changes nothing that is live - see `discard_collection_item_draft`.
   */
  discardCollectionItemDraft(name: string, id: number): Observable<void> {
    return this.http.delete<void>(apiUrl(`/models/collections/${name}/items/${id}/draft`));
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
    return this.http.post<number>(
      apiUrl(`/models/collections/${name}/items/${id}/duplicate`),
      null,
    );
  }

  publishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(
      apiUrl(`/models/collections/${name}/items/${id}/publish`),
      null,
    );
  }

  unpublishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.http.post<ItemMetadata>(
      apiUrl(`/models/collections/${name}/items/${id}/unpublish`),
      null,
    );
  }

  /** Mints a link that shows this working copy to someone without an account. */
  /**
   * The content that points at this item, by the relation index.
   *
   * Read with a token: the management API shows drafts, which is what an editor needs to see
   * before deleting something.
   */
  itemReferences(name: string, id: number): Observable<RelationReference[]> {
    return this.http.get<RelationReference[]>(
      apiUrl(`/models/collections/${name}/items/${id}/references`),
    );
  }

  createPreviewLink(name: string, id: number): Observable<PreviewLink> {
    return this.http.post<PreviewLink>(
      apiUrl(`/models/collections/${name}/items/${id}/preview-link`),
      null,
    );
  }
}
