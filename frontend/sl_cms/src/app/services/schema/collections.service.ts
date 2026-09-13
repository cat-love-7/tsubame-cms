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
}
