import { Injectable, inject } from '@angular/core';
import {
  ItemMetadata,
  ItemMetadataMap,
  ItemStatus,
  ItemStatusOutcome,
} from 'app/models/item-status';
import { PreviewLink } from 'app/models/links';
import { RelationReference } from 'app/models/relations';

import { CollectionSchema } from 'app/models/schema/collection';
import { CollectionItemPage, CollectionValue } from 'app/models/values/collection';
import { FieldValue } from 'app/models/values/fields';
import { CollectionRepository } from 'app/repositories/schema/collections.repository';
import { Observable } from 'rxjs';

@Injectable({
  providedIn: 'root',
})
export class CollectionsService {
  private collectionRepository = inject(CollectionRepository);
  getAllCollectionNames(): Observable<string[]> {
    return this.collectionRepository.getAllCollectionNames();
  }
  getCollectionSchema(name: string): Observable<CollectionSchema> {
    return this.collectionRepository.getCollectionSchema(name);
  }
  /** The titles of the named items, keyed by id written as a string. */
  getItemTitles(name: string, ids: number[]): Observable<Record<string, FieldValue>> {
    return this.collectionRepository.getCollectionItemTitles(name, ids);
  }
  createCollection(name: string, schema: CollectionSchema): Observable<void> {
    return this.collectionRepository.createCollection(name, schema);
  }
  updateCollectionSchema(name: string, schema: CollectionSchema): Observable<void> {
    return this.collectionRepository.updateCollectionSchema(name, schema);
  }
  deleteCollection(name: string): Observable<void> {
    return this.collectionRepository.deleteCollection(name);
  }

  // ---- items ---------------------------------------------------------------

  /** One page of items, with the total the server reports for the collection. */
  listCollectionItemsPage(
    name: string,
    page: { limit: number; offset: number; sort?: string },
  ): Observable<CollectionItemPage> {
    return this.collectionRepository.listCollectionItemsPage(
      name,
      page.limit,
      page.offset,
      page.sort,
    );
  }

  getCollectionItem(name: string, id: number): Observable<CollectionValue> {
    return this.collectionRepository.getCollectionItem(name, id);
  }

  createCollectionItem(name: string, values: CollectionValue): Observable<number> {
    return this.collectionRepository.createCollectionItem(name, values);
  }

  updateCollectionItem(name: string, id: number, values: CollectionValue): Observable<void> {
    return this.collectionRepository.updateCollectionItem(name, id, values);
  }

  deleteCollectionItem(name: string, id: number): Observable<void> {
    return this.collectionRepository.deleteCollectionItem(name, id);
  }

  // ---- draft / published ---------------------------------------------------

  listItemMetadata(name: string): Observable<ItemMetadataMap> {
    return this.collectionRepository.listItemMetadata(name);
  }

  getItemMetadata(name: string, id: number): Observable<ItemMetadata> {
    return this.collectionRepository.getItemMetadata(name, id);
  }

  /** Publish or unpublish a batch of items, answering per item. */
  setItemsStatus(name: string, ids: number[], status: ItemStatus): Observable<ItemStatusOutcome[]> {
    return this.collectionRepository.setItemsStatus(name, ids, status);
  }

  /** Copy an item; the answer is the new item's id. */
  duplicateItem(name: string, id: number): Observable<number> {
    return this.collectionRepository.duplicateItem(name, id);
  }

  publishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.collectionRepository.publishItem(name, id);
  }

  unpublishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.collectionRepository.unpublishItem(name, id);
  }

  /** The content that points at this item, for the references panel. */
  itemReferences(name: string, id: number): Observable<RelationReference[]> {
    return this.collectionRepository.itemReferences(name, id);
  }

  createPreviewLink(name: string, id: number): Observable<PreviewLink> {
    return this.collectionRepository.createPreviewLink(name, id);
  }
}
