import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { ItemMetadata } from 'app/models/item-status';
import { PreviewLink } from 'app/models/links';
import { RelationReference } from 'app/models/relations';

import { CollectionSchema } from 'app/models/schema/collection';
import { SchemaSettings } from 'app/models/schema/settings';
import { FieldValue } from 'app/models/values/fields';
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

  /** The title of every page that has one, keyed by page name. */
  getPageTitles(): Observable<Record<string, FieldValue>> {
    return this.pages.getPageTitles();
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

  /** What the page is told about itself, apart from its fields. */
  getPageSettings(name: string): Observable<SchemaSettings> {
    return this.pages.getPageSettings(name);
  }
  updatePageSettings(name: string, settings: SchemaSettings): Observable<void> {
    return this.pages.updatePageSettings(name, settings);
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

  /** What the site serves for this page, as opposed to the working copy the editor holds. */
  getPublishedPageItem(name: string): Observable<ContentValue> {
    return this.pages.getPublishedPageItem(name);
  }

  /** Throw the working copy away: the page goes back to what the site is serving. */
  discardPageDraft(name: string): Observable<void> {
    return this.pages.discardPageDraft(name);
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

  /** The content that points at this page, for the references panel. */
  pageReferences(name: string): Observable<RelationReference[]> {
    return this.pages.pageReferences(name);
  }

  createPreviewLink(name: string): Observable<PreviewLink> {
    return this.pages.createPreviewLink(name);
  }
}
