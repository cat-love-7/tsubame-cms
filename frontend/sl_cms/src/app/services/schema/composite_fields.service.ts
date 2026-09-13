import { Injectable } from '@angular/core';
import { CompositeFieldSchema } from 'app/models/schema/fields';
import { CompositeFieldRepository } from 'app/repositories/schema/composite_fields.repository';
import { Observable } from 'rxjs';

@Injectable({
  providedIn: 'root',
})
export class CompositeFieldsService {
  constructor(private compositeFieldRepository: CompositeFieldRepository) {}
  getAllCompositeFields(): Observable<{[name:string]:CompositeFieldSchema}> {
    return this.compositeFieldRepository.getAllCompositeFields();
  }
}
