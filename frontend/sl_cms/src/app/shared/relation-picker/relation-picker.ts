import { Component, EventEmitter, Input, OnInit, Output, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
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
 * Candidates are loaded when the panel is opened, not when the field is drawn: a content form can
 * hold several relation fields, and only the one an editor opens is worth a request.
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
    MessagePipe,
    TranslocoPipe,
  ],
  templateUrl: './relation-picker.html',
  styleUrl: './relation-picker.scss',
})
export class RelationPicker implements OnInit {
  private collections = inject(CollectionsService);
  private pages = inject(SinglePagesService);

  /** What the relation points at: a collection's items, or a single page. */
  @Input({ required: true }) target!: RelationTarget;
  /** What is already referenced, so the list can show it as picked. */
  @Input() selected: RelationRef[] = [];
  /** Whether several may be referenced, which is the field's own `has_many`. */
  @Input() multiple = false;
  @Output() toggled = new EventEmitter<RelationRef>();
  @Output() closed = new EventEmitter<void>();

  public candidates = signal<Candidate[]>([]);
  public filter = signal('');
  public loading = signal(true);
  public error = signal<Message | null>(null);

  ngOnInit() {
    this.load();
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

  private load() {
    if (this.target.name.trim() === '') {
      // A relation whose target has not been chosen yet: nothing to offer, and no request to make.
      this.loading.set(false);
      return;
    }
    if (this.target.kind === 'single_page') {
      this.loadPages();
      return;
    }
    this.loadItems();
  }

  /** A collection's items, with the name each one's schema gives it. */
  private loadItems() {
    this.collections
      .listCollectionItemsPage(this.target.name, { limit: CANDIDATES, offset: 0 })
      .subscribe({
        next: (page) => {
          const ids = page.items.map(([id]) => id);
          this.collections.getItemTitles(this.target.name, ids).subscribe({
            next: (titles) => {
              this.candidates.set(
                page.items.map(([id]) => ({
                  reference: { target: this.target.name, item: id },
                  label: labelOf(titles[String(id)], `${this.target.name} #${id}`),
                })),
              );
              this.loading.set(false);
            },
            // The items are here and their names are not: the reference is what is left, and it is
            // still something to pick.
            error: () => this.showItemsWithoutTitles(ids),
          });
        },
        error: (e) => this.failed(e),
      });
  }

  private showItemsWithoutTitles(ids: number[]) {
    this.candidates.set(
      ids.map((id) => ({
        reference: { target: this.target.name, item: id },
        label: `${this.target.name} #${id}`,
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
