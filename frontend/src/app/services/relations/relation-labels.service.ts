import { Injectable, inject } from '@angular/core';
import { Observable, finalize, forkJoin, map, of, shareReplay } from 'rxjs';

import { FieldValue, RelationRef, formatFieldValue } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single-pages.service';

/**
 * What a reference is called: the field its target's schema names as the title, rendered the way
 * the screens render any value.
 *
 * Asked for per screen rather than cached: a title is a value an editor may have just changed, and
 * a name that lags behind a rename is worse than one more request. What is asked for is bounded by
 * what is on screen - a page of rows, or the references of one item.
 *
 * What is shared is only what is already on the way (see `inFlight`): an array of composites holds
 * one relation field per element, and every one of them asks about the same target at the same
 * moment.
 */
@Injectable({
  providedIn: 'root',
})
export class RelationLabelsService {
  private collections = inject(CollectionsService);
  private pages = inject(SinglePagesService);

  /**
   * The names of these references, keyed the way `referenceKey` keys them.
   *
   * A target collection that names no title field, or an item that is gone, is simply absent: the
   * caller falls back to the reference itself.
   */
  labelsFor(references: RelationRef[]): Observable<Map<string, string>> {
    const byCollection = new Map<string, number[]>();
    let pagesWanted = false;
    for (const reference of references) {
      if (typeof reference.item === 'number') {
        const ids = byCollection.get(reference.target) ?? [];
        if (!ids.includes(reference.item)) {
          ids.push(reference.item);
        }
        byCollection.set(reference.target, ids);
      } else {
        pagesWanted = true;
      }
    }
    if (byCollection.size === 0 && !pagesWanted) {
      return of(new Map<string, string>());
    }
    // One request per target collection, and one for every page: what the screens ask about is
    // what they are showing, not a collection's worth of rows.
    const wanted: Observable<Map<string, string>>[] = [...byCollection].map(([name, ids]) =>
      this.titlesFor(name, ids),
    );
    if (pagesWanted) {
      wanted.push(this.pageTitles());
    }
    return forkJoin(wanted).pipe(
      map((answers) => {
        const labels = new Map<string, string>();
        for (const answer of answers) {
          for (const [key, value] of answer) {
            labels.set(key, value);
          }
        }
        return labels;
      }),
    );
  }

  /**
   * The requests already on the way, keyed by what they ask for.
   *
   * An entry lives only until its answer arrives, so this is not a cache: a question asked
   * afterwards is asked again, which is what keeps a rename from being answered with a stale name.
   */
  private inFlight = new Map<string, Observable<Map<string, string>>>();

  /** One collection's titles, sharing the request with whoever is already waiting on it. */
  private titlesFor(name: string, ids: number[]): Observable<Map<string, string>> {
    return this.share(
      `collection:${name}:${ids.join(',')}`,
      this.collections
        .getItemTitles(name, ids)
        .pipe(map((titles) => entriesOf(titles, (id) => `collection:${name}:${id}`))),
    );
  }

  /** The pages' titles, the same way. */
  private pageTitles(): Observable<Map<string, string>> {
    return this.share(
      'single_pages',
      this.pages.getPageTitles().pipe(map((titles) => entriesOf(titles, (name) => `page:${name}`))),
    );
  }

  private share(
    key: string,
    request: Observable<Map<string, string>>,
  ): Observable<Map<string, string>> {
    const known = this.inFlight.get(key);
    if (known) {
      return known;
    }
    const shared = request.pipe(
      finalize(() => this.inFlight.delete(key)),
      shareReplay({ bufferSize: 1, refCount: false }),
    );
    this.inFlight.set(key, shared);
    return shared;
  }
}

/** The answers as rendered names, keyed by what they are the title of. */
function entriesOf(
  titles: Record<string, FieldValue>,
  keyOf: (key: string) => string,
): Map<string, string> {
  const entries = new Map<string, string>();
  for (const [key, value] of Object.entries(titles)) {
    entries.set(keyOf(key), formatFieldValue(value));
  }
  return entries;
}
