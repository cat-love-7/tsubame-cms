import { HttpClient } from "@angular/common/http";
import { Injectable } from "@angular/core";
import { Observable } from "rxjs/internal/Observable";
import { CollectionSchema } from "app/models/schema/collection";
import { CompositeFieldSchema } from "app/models/schema/fields";

@Injectable()
export class CompositeFieldRepository {
  constructor(private http: HttpClient) {}
  getAllCompositeFields(): Observable<{[name:string]:CompositeFieldSchema}> {
    return this.http.get<{[name:string]:CompositeFieldSchema}>('/api/models/composite_fields');
  }
  getCompositeFieldSchema(name: string): Observable<CollectionSchema> {
    return this.http.get<CollectionSchema>(`/api/models/collections/${name}`);
  }
  updateCompositeFieldSchema(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.put<void>(`/api/models/composite_fields/${name}`, schema);
  }
  createCompositeField(name: string, schema: CollectionSchema): Observable<void> {
    return this.http.post<void>(`/api/models/composite_fields/${name}`, schema );
  }
}