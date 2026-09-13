import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';

import { ItemMetadata } from 'app/models/item-status';
import { CollectionSchema } from 'app/models/schema/collection';
import { ContentValue } from 'app/models/values/collection';
import { SinglePageRepository } from 'app/repositories/schema/single_pages.repository';

@Injectable({
  providedIn: 'root',
})
export class SinglePagesService {
  constructor(private pages: SinglePageRepository) {}

  listPageNames(): Observable<string[]> {
    return this.pages.listPageNames();
  }

  getPageSchema(name: string): Observable<CollectionSchema> {
    return this.pages.getPageSchema(name);
  }

  createPage(name: string, schema: CollectionSchema): Observable<void> {
    return this.pages.createPage(name, schema);
  }

  updatePageSchema(name: string, schema: CollectionSchema): Observable<void> {
    return this.pages.updatePageSchema(name, schema);
  }

  deletePage(name: string): Observable<void> {
    return this.pages.deletePage(name);
  }

  getPageItem(name: string): Observable<ContentValue> {
    return this.pages.getPageItem(name);
  }

  updatePageItem(name: string, values: ContentValue): Observable<void> {
    return this.pages.updatePageItem(name, values);
  }

  // ---- draft / published ---------------------------------------------------

  getPageMetadata(name: string): Observable<ItemMetadata> {
    return this.pages.getPageMetadata(name);
  }

  publishPage(name: string): Observable<ItemMetadata> {
    return this.pages.publishPage(name);
  }

  unpublishPage(name: string): Observable<ItemMetadata> {
    return this.pages.unpublishPage(name);
  }
}
