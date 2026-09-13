import { HttpClient } from "@angular/common/http";
import { Injectable } from "@angular/core";
import { Observable } from "rxjs/internal/Observable";
import { CollectionSchema } from "app/models/schema/collection";

@Injectable()
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
    return this.http.post<void>(`/api/models/collections/${name}/schema`, schema );
  }
}