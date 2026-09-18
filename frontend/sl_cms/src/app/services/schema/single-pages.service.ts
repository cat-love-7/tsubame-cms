import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { ItemMetadata } from 'app/models/item-status';
import { PreviewLink } from 'app/models/links';

import { CollectionSchema } from 'app/models/schema/collection';
import { ContentValue } from 'app/models/values/single-page';

import { SinglePageRepository } from 'app/repositories/schema/single-pages.repository';

@Injectable({
  providedIn: 'root',
})
export class SinglePagesService {
  private pages = inject(SinglePageRepository);

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

  /** The state of every page, keyed by name, for the list screen. */
  listItemMetadata(): Observable<{ [name: string]: ItemMetadata }> {
    return this.pages.listItemMetadata();
  }

  publishPage(name: string): Observable<ItemMetadata> {
    return this.pages.publishPage(name);
  }

  unpublishPage(name: string): Observable<ItemMetadata> {
    return this.pages.unpublishPage(name);
  }

  createPreviewLink(name: string): Observable<PreviewLink> {
    return this.pages.createPreviewLink(name);
  }
}
