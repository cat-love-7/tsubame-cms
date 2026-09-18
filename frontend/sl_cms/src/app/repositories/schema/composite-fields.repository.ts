import { HttpClient } from '@angular/common/http';
import { apiUrl } from 'app/core/api-url';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { CompositeFieldDefinition } from 'app/models/schema/collection';

/**
 * Composite fields: reusable groups of fields that a collection or single page schema
 * references by id.
 */
@Injectable({
  providedIn: 'root',
})
export class CompositeFieldRepository {
  private http = inject(HttpClient);

  /** Every definition, keyed by id. */
  getAllCompositeFields(): Observable<{ [id: string]: CompositeFieldDefinition }> {
    return this.http.get<{ [id: string]: CompositeFieldDefinition }>(
      apiUrl('/models/composite_fields'),
    );
  }

  getCompositeFieldSchema(id: string): Observable<CompositeFieldDefinition> {
    return this.http.get<CompositeFieldDefinition>(apiUrl(`/models/composite_fields/${id}`));
  }

  createCompositeField(id: string, schema: CompositeFieldDefinition): Observable<void> {
    return this.http.post<void>(apiUrl(`/models/composite_fields/${id}`), schema);
  }

  updateCompositeFieldSchema(id: string, schema: CompositeFieldDefinition): Observable<void> {
    return this.http.put<void>(apiUrl(`/models/composite_fields/${id}`), schema);
  }

  deleteCompositeField(id: string): Observable<void> {
    return this.http.delete<void>(apiUrl(`/models/composite_fields/${id}`));
  }
}
