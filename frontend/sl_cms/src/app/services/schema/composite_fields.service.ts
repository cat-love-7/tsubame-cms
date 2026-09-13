import { Injectable } from '@angular/core';
import { Observable, shareReplay } from 'rxjs';

import { CompositeFieldDefinition } from 'app/models/schema/collection';
import { CompositeFieldRepository } from 'app/repositories/schema/composite_fields.repository';

@Injectable({
  providedIn: 'root',
})
export class CompositeFieldsService {
  constructor(private compositeFieldRepository: CompositeFieldRepository) {}

  private all$?: Observable<{ [id: string]: CompositeFieldDefinition }>;

  /**
   * Every composite definition, cached.
   *
   * A content form can hold many composite fields and each nested one needs the same map,
   * so fetching per component would mean a burst of identical requests.
   */
  getAllCompositeFields(): Observable<{ [id: string]: CompositeFieldDefinition }> {
    this.all$ ??= this.compositeFieldRepository.getAllCompositeFields().pipe(shareReplay(1));
    return this.all$;
  }

  /**
   * Drop the cache so the next read picks up a changed definition. Call this after
   * creating, updating or deleting one.
   */
  invalidate() {
    this.all$ = undefined;
  }

  getCompositeFieldSchema(id: string): Observable<CompositeFieldDefinition> {
    return this.compositeFieldRepository.getCompositeFieldSchema(id);
  }

  createCompositeField(id: string, schema: CompositeFieldDefinition): Observable<void> {
    return this.compositeFieldRepository.createCompositeField(id, schema);
  }

  updateCompositeFieldSchema(id: string, schema: CompositeFieldDefinition): Observable<void> {
    return this.compositeFieldRepository.updateCompositeFieldSchema(id, schema);
  }

  deleteCompositeField(id: string): Observable<void> {
    return this.compositeFieldRepository.deleteCompositeField(id);
  }
}
