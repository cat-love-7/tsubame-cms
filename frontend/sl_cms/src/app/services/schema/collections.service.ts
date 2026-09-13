import { Injectable } from '@angular/core';
import { CollectionSchema } from 'app/models/schema/collection';
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
}
