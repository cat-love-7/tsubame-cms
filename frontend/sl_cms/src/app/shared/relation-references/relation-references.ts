import { Component, Input, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { TranslocoPipe } from '@jsverse/transloco';
import { forkJoin, of } from 'rxjs';
import { catchError, map, switchMap } from 'rxjs/operators';

import { RelationReference } from 'app/models/relations';
import { isRelationFieldSchema } from 'app/models/schema/fields';
import { FieldValue, formatFieldValue } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single-pages.service';

/** One heading of the panel: the name the other side gives the relation, and what is under it. */
interface ReferenceGroup {
  heading: string;
  rows: ReferenceRow[];
}

interface ReferenceRow {
  label: string;
  link: string[];
}

/**
 * What points at this item: the inverse of its own references, which is a question an editor asks
 * before deleting something or before wondering why a list looks the way it does.
 *
 * The heading is what the *other* schema calls the relation (`inverse_name`), because that is the
 * name that side uses: a category's screen says which articles name it, not "posts where a
 * relation field points here". A schema that names nothing falls back to the collection's own name,
 * which is still better than a list with no heading.
 *
 * It loads when it is opened, not when the screen is: what points at an item is a question an
 * editor asks occasionally, and every content screen would otherwise pay for it (the picker is
 * lazy for the same reason).
 */
@Component({
  selector: 'app-relation-references',
  imports: [RouterLink, TranslocoPipe],
  templateUrl: './relation-references.html',
  styleUrl: './relation-references.scss',
})
export class RelationReferences {
  private collections = inject(CollectionsService);
  private pages = inject(SinglePagesService);

  /** What the panel is about: a collection's item, or a single page. */
  @Input({ required: true }) kind!: 'collection_item' | 'single_page';
  @Input({ required: true }) name!: string;
  /** The item's id; absent for a single page, which has one item and no id. */
  @Input() item?: number | null;

  public groups = signal<ReferenceGroup[]>([]);
  public loading = signal(false);
  public failed = signal(false);
  /** Whether the panel is open, and whether it has ever been (so it asks once). */
  public open = signal(false);
  private asked = false;

  /** Open it, and ask the first time it is opened. */
  public toggle() {
    this.open.set(!this.open());
    if (this.open() && !this.asked) {
      this.asked = true;
      this.loading.set(true);
      this.load();
    }
  }

  private load() {
    const references =
      this.kind === 'single_page' || this.item == null
        ? this.pages.pageReferences(this.name)
        : this.collections.itemReferences(this.name, this.item);
    references
      .pipe(
        switchMap((found) => (found.length === 0 ? of([]) : this.group(found))),
        catchError(() => {
          // A panel that cannot load says so; it must not look like "nothing points here".
          this.failed.set(true);
          return of([]);
        }),
      )
      .subscribe((groups) => {
        this.groups.set(groups);
        this.loading.set(false);
      });
  }

  /** The referrers, grouped by the schema that holds them, with each group's heading. */
  private group(found: RelationReference[]) {
    const byOwner = new Map<string, RelationReference[]>();
    for (const reference of found) {
      const key = `${reference.kind}:${reference.name}`;
      byOwner.set(key, [...(byOwner.get(key) ?? []), reference]);
    }
    return forkJoin(
      [...byOwner].map(([key, references]) => {
        const [kind, name] = key.split(':') as ['collection_item' | 'single_page', string];
        return this.heading(kind, name).pipe(
          switchMap((heading) =>
            this.rows(kind, name, references).pipe(map((rows) => ({ heading, rows }))),
          ),
        );
      }),
    );
  }

  /**
   * What the referrer calls this relation: its schema's `inverse_name` for a field pointing here,
   * and its own name when it names nothing.
   */
  private heading(kind: 'collection_item' | 'single_page', name: string) {
    const schema =
      kind === 'single_page'
        ? this.pages.getPageSchema(name)
        : this.collections.getCollectionSchema(name);
    return schema.pipe(
      map((fields) => {
        const pointsHere = fields.find(
          (field) =>
            isRelationFieldSchema(field.field_type) &&
            (
              field.field_type as {
                Relation: { target: { kind: string; name: string }; inverse_name?: string };
              }
            ).Relation.target.kind ===
              (this.kind === 'single_page' ? 'single_page' : 'collection') &&
            (field.field_type as { Relation: { target: { name: string } } }).Relation.target
              .name === this.name,
        );
        const inverse = pointsHere
          ? (pointsHere.field_type as { Relation: { inverse_name?: string | null } }).Relation
              .inverse_name
          : null;
        return inverse?.trim() ? inverse : name;
      }),
      // A schema that cannot be read still leaves the referrers worth showing.
      catchError(() => of(name)),
    );
  }

  /** The rows: each referrer under its name, linked to the screen that edits it. */
  private rows(
    kind: 'collection_item' | 'single_page',
    name: string,
    references: RelationReference[],
  ) {
    if (kind === 'single_page') {
      const rows = references
        .filter((reference) => reference.name === name)
        .map((reference) => ({
          label: reference.name,
          link: ['/single-pages', reference.name],
        }));
      return of(rows);
    }
    const ids = references
      .map((reference) => reference.item)
      .filter((item): item is number => typeof item === 'number');
    if (ids.length === 0) {
      return of([]);
    }
    return this.collections.getItemTitles(name, ids).pipe(
      map((titles) =>
        references.map((reference) => ({
          label: labelOf(titles[String(reference.item)], reference.item),
          link: ['/collections', name, 'edit', String(reference.item)],
        })),
      ),
      catchError(() =>
        of(
          references.map((reference) => ({
            label: `${name} #${reference.item}`,
            link: ['/collections', name, 'edit', String(reference.item)],
          })),
        ),
      ),
    );
  }
}

/** What to call a referrer: the name its schema gives it, and the id when there is no name. */
function labelOf(title: FieldValue | undefined, item: number | null | undefined): string {
  if (title === undefined || title === null) {
    return `#${item}`;
  }
  const rendered = formatFieldValue(title);
  return rendered === '' ? `#${item}` : rendered;
}
