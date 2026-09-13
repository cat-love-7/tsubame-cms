import { Injectable } from '@angular/core';
import { ItemMetadata, ItemMetadataMap, PreviewLink } from 'app/models/item-status';
import { CollectionSchema } from 'app/models/schema/collection';
import { CollectionItemPage, CollectionValue } from 'app/models/values/collection';
import { CollectionRepository } from 'app/repositories/schema/collections.repository';
import { Observable } from 'rxjs';

@Injectable({
  providedIn: 'root',
})
export class CollectionsService {
  constructor(private collectionRepository: CollectionRepository) {}
  getAllCollectionNames(): Observable<string[]> {
    return this.collectionRepository.getAllCollectionNames();
  }
  getCollectionSchema(name: string): Observable<CollectionSchema> {
    return this.collectionRepository.getCollectionSchema(name);
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
    page: { limit: number; offset: number },
  ): Observable<CollectionItemPage> {
    return this.collectionRepository.listCollectionItemsPage(name, page.limit, page.offset);
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

  publishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.collectionRepository.publishItem(name, id);
  }

  unpublishItem(name: string, id: number): Observable<ItemMetadata> {
    return this.collectionRepository.unpublishItem(name, id);
  }

  createPreviewLink(name: string, id: number): Observable<PreviewLink> {
    return this.collectionRepository.createPreviewLink(name, id);
  }
}
