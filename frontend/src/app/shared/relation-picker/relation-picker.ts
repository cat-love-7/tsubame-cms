import { Component, EventEmitter, Input, OnInit, Output, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { TranslocoPipe } from '@jsverse/transloco';
import { forkJoin } from 'rxjs';

import { Message, MessagePipe, failure } from 'app/core/i18n/message';
import { RelationTarget } from 'app/models/schema/fields';
import { FieldValue, RelationRef, formatFieldValue, referenceKey } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single-pages.service';

/** One item that could be referenced: the reference itself, and what to call it. */
interface Candidate {
  reference: RelationRef;
  label: string;
}

/**
 * How many candidates are listed at once.
 *
 * A picker is for choosing one of the things an editor has in mind, not for browsing a collection:
 * the ones that are not listed are a filter away (and the JSON editor is the way to reference
 * something by hand, which is what a migration does anyway).
 */
const CANDIDATES = 100;

/**
 * Choosing what a relation points at, by name.
 *
 * A field declares one target for a `Relation` and one per item type for an array of them, so the
 * picker offers whichever target is chosen and switches between them (with one target there is
 * nothing to switch). Candidates are loaded when the panel is opened, not when the field is drawn:
 * a content form can hold several relation fields, and only the one an editor opens is worth a
 * request.
 */
@Component({
  selector: 'app-relation-picker',
  imports: [
    FormsModule,
    MatButtonModule,
    MatCheckboxModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSelectModule,
    MessagePipe,
    TranslocoPipe,
  ],
  templateUrl: './relation-picker.html',
  styleUrl: './relation-picker.scss',
})
export class RelationPicker implements OnInit {
  private collections = inject(CollectionsService);
  private pages = inject(SinglePagesService);

  /** What the relation may point at: a collection's items, a single page, or one of each. */
  @Input({ required: true }) targets: RelationTarget[] = [];
  /** What is already referenced, so the list can show it as picked. */
  @Input() selected: RelationRef[] = [];
  /** Whether several may be referenced, which is an array of relations. */
  @Input() multiple = false;
  @Output() toggled = new EventEmitter<RelationRef>();
  @Output() closed = new EventEmitter<void>();

  public candidates = signal<Candidate[]>([]);
  public filter = signal('');
  public loading = signal(true);
  public error = signal<Message | null>(null);
  /** Which target's candidates are listed; the first usable one until the editor picks another. */
  public chosenTarget = signal<RelationTarget | null>(null);

  ngOnInit() {
    const first = this.usableTargets()[0] ?? null;
    this.chosenTarget.set(first);
    this.load(first);
  }

  /** The declared targets, minus the ones whose name has not been chosen yet. */
  usableTargets(): RelationTarget[] {
    return this.targets.filter((target) => target.name.trim() !== '');
  }

  /** The key a target is identified by, exposed for the template. */
  targetKey(target: RelationTarget): string {
    return `${target.kind}:${target.name}`;
  }

  /** The chosen target as a key, for the selector. */
  chosenTargetKey(): string {
    const target = this.chosenTarget();
    return target === null ? '' : this.targetKey(target);
  }

  /** Switch which target's candidates are listed. */
  chooseTarget(key: string) {
    const target = this.usableTargets().find((candidate) => this.targetKey(candidate) === key);
    this.chosenTarget.set(target ?? null);
    this.load(target ?? null);
  }

  /** The candidates the filter leaves, in the order the target lists them. */
  visible(): Candidate[] {
    const wanted = this.filter().trim().toLowerCase();
    if (wanted === '') {
      return this.candidates();
    }
    return this.candidates().filter((candidate) => candidate.label.toLowerCase().includes(wanted));
  }

  isPicked(candidate: Candidate): boolean {
    const key = referenceKey(candidate.reference);
    return this.selected.some((reference) => referenceKey(reference) === key);
  }

  toggle(candidate: Candidate) {
    this.toggled.emit(candidate.reference);
  }

  private load(target: RelationTarget | null) {
    this.candidates.set([]);
    this.error.set(null);
    if (target === null || target.name.trim() === '') {
      // A relation whose target has not been chosen yet: nothing to offer, and no request to make.
      this.loading.set(false);
      return;
    }
    this.loading.set(true);
    if (target.kind === 'single_page') {
      this.loadPages();
      return;
    }
    this.loadItems(target);
  }

  /** A collection's items, with the name each one's schema gives it. */
  private loadItems(target: RelationTarget) {
    this.collections
      .listCollectionItemsPage(target.name, { limit: CANDIDATES, offset: 0 })
      .subscribe({
        next: (page) => {
          const ids = page.items.map(([id]) => id);
          this.collections.getItemTitles(target.name, ids).subscribe({
            next: (titles) => {
              this.candidates.set(
                page.items.map(([id]) => ({
                  reference: { target: target.name, item: id },
                  label: labelOf(titles[String(id)], `${target.name} #${id}`),
                })),
              );
              this.loading.set(false);
            },
            // The items are here and their names are not: the reference is what is left, and it is
            // still something to pick.
            error: () => this.showItemsWithoutTitles(target.name, ids),
          });
        },
        error: (e) => this.failed(e),
      });
  }

  private showItemsWithoutTitles(name: string, ids: number[]) {
    this.candidates.set(
      ids.map((id) => ({
        reference: { target: name, item: id },
        label: `${name} #${id}`,
      })),
    );
    this.loading.set(false);
  }

  /** Pages: a page has one item, so the page is the candidate. */
  private loadPages() {
    forkJoin({
      names: this.pages.listPageNames(),
      titles: this.pages.getPageTitles(),
    }).subscribe({
      next: ({ names, titles }) => {
        this.candidates.set(
          names.map((name) => ({
            reference: { target: name },
            label: labelOf(titles[name], name),
          })),
        );
        this.loading.set(false);
      },
      error: (e) => this.failed(e),
    });
  }

  private failed(error: unknown) {
    this.error.set(failure('content.failedToLoadItems', error));
    this.loading.set(false);
  }
}

/** What to call a candidate: the title its schema names, and the reference when there is none. */
function labelOf(title: FieldValue | undefined, fallback: string): string {
  if (title === undefined || title === null) {
    return fallback;
  }
  const rendered = formatFieldValue(title);
  return rendered === '' ? fallback : rendered;
}
