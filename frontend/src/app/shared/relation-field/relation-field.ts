import {
  Component,
  DestroyRef,
  EventEmitter,
  Input,
  OnChanges,
  OnInit,
  Output,
  SimpleChanges,
  inject,
  signal,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatChipsModule } from '@angular/material/chips';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe } from '@jsverse/transloco';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';

import { Message, t } from 'app/core/i18n/message';
import { FieldSchema, RelationTarget } from 'app/models/schema/fields';
import {
  FieldValue,
  RelationRef,
  referenceKey,
  referenceName,
  relationRefsOf,
} from 'app/models/values/fields';
import { RelationLabelsService } from 'app/services/relations/relation-labels.service';
import { RelationPicker } from 'app/shared/relation-picker/relation-picker';

/**
 * A relation: what this field points at, by name.
 *
 * One widget draws both shapes. A `Relation` holds one reference - `{target, item}` for an item,
 * `{target}` for a page, or `null` - and an `Array([Relation(…)])` holds a list of the same
 * objects, which is what several references are. `targets` is what the field's own schema declares:
 * one for a relation, one per item type for an array.
 *
 * The chips are the value, and the JSON box (which is what the API takes) stays behind a toggle for
 * a value the picker cannot express.
 */
@Component({
  selector: 'app-relation-field',
  imports: [
    RelationPicker,
    MatChipsModule,
    MatTooltipModule,
    FormsModule,
    MatButtonModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    TranslocoPipe,
  ],
  templateUrl: './relation-field.html',
  styleUrl: './relation-field.scss',
})
export class RelationField implements OnInit, OnChanges {
  private relationLabels = inject(RelationLabelsService);
  private destroyRef = inject(DestroyRef);

  @Input({ required: true }) field!: FieldSchema;
  /** What the value may name: the field's target, or an array's one target per item type. */
  @Input() targets: RelationTarget[] = [];
  /** Whether the value is a list of references (an `Array` of relations) or one reference. */
  @Input() multiple = false;
  @Input() value: FieldValue = null;
  /** Renders the chips read-only, for the schema editor's preview. */
  @Input() disabled = false;
  @Input() labelId = '';
  @Output() valueChange = new EventEmitter<FieldValue>();
  @Output() errorChange = new EventEmitter<Message | null>();

  /** The JSON box's text, which the references are typed into. */
  public arrayText = '';
  /** The JSON box is a fallback, so it starts closed. */
  public jsonMode = signal(false);
  /** Whether the reference picker is open under this field. */
  public relationPickerOpen = signal(false);

  /**
   * The names of the references this field holds, when the target's schema names them.
   *
   * The box edits the references themselves, so this is what tells an editor what they just wrote:
   * `categories #3` is a reference, and `技術` is the item it points at.
   */
  public referenceNames = signal<string[]>([]);

  /** The labels the target's schema answered, by reference. */
  private labels = signal<ReadonlyMap<string, string>>(new Map());

  /** The last value this component emitted, so its own output is not mistaken for new input
   * (which would reset the JSON box mid-typing). */
  private lastEmitted: FieldValue | undefined = undefined;

  ngOnInit() {
    this.loadReferenceNames(this.value);
    this.syncJsonBox();
  }

  ngOnChanges(changes: SimpleChanges) {
    if (changes['value'] || changes['field'] || changes['targets'] || changes['multiple']) {
      this.loadReferenceNames(this.value);
      this.syncJsonBox();
    }
  }

  /**
   * The references this field holds, as a list.
   *
   * Read from the value rather than kept beside it: the chips, the picker and the JSON box all edit
   * the one value, and a second copy is how the two drift apart. A single reference is a list of
   * one here, which is what lets both shapes share the chips.
   */
  relationRefs(): RelationRef[] {
    return relationRefsOf(this.value);
  }

  /** Whether this field holds one reference, which is what a pick replaces rather than adds to. */
  relationIsSingle(): boolean {
    return !this.multiple;
  }

  /**
   * Add the reference the picker chose, or take it away when it was already there.
   *
   * A set: clicking what is picked is how it is unpicked. A single reference replaces what it held
   * rather than refusing the pick - the picker has already said which one it wants.
   */
  toggleReference(reference: RelationRef) {
    const refs = this.relationRefs();
    const key = referenceKey(reference);
    const without = refs.filter((candidate) => referenceKey(candidate) !== key);
    const alreadyPicked = refs.length !== without.length;
    const next = alreadyPicked
      ? without
      : this.relationIsSingle()
        ? [reference]
        : [...without, reference];
    this.errorChange.emit(null);
    this.emitRefs(next);
  }

  /**
   * Move a reference one place.
   *
   * The order is the value: a site showing "featured articles" shows them in the order the editor
   * put them in, so this is a change to the value like any other - and the index does not care
   * (it holds a set of references), so nothing else has to move.
   */
  moveReference(index: number, delta: number) {
    const references = [...this.relationRefs()];
    const target = index + delta;
    if (target < 0 || target >= references.length) {
      return;
    }
    [references[index], references[target]] = [references[target], references[index]];
    this.errorChange.emit(null);
    this.emitRefs(references);
  }

  /** The key a reference is tracked by, exposed for the template. */
  public referenceKey = referenceKey;

  /** What to call a reference in a chip: the target's title, or the reference itself. */
  referenceLabel(reference: RelationRef): string {
    return referenceName(reference, this.labels());
  }

  /**
   * Take the references the JSON box holds.
   *
   * The box is the fallback for what the picker cannot express, so what it holds is checked here
   * against the same rules the server applies, and the value only moves when they pass. A `Relation`
   * takes one object or `null`; an array of them takes an array.
   */
  onJsonChange(text: string) {
    this.arrayText = text;
    const trimmed = text.trim();
    if (trimmed === '') {
      this.errorChange.emit(null);
      this.emitRefs([]);
      return;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(trimmed);
    } catch {
      this.errorChange.emit(t('content.invalidJson', { field: this.field.name }));
      return;
    }
    if (this.multiple) {
      if (!Array.isArray(parsed)) {
        this.errorChange.emit(t('content.expectedJsonArray', { field: this.field.name }));
        return;
      }
    } else if (parsed === null) {
      this.errorChange.emit(null);
      this.emitRefs([]);
      return;
    } else if (typeof parsed !== 'object' || Array.isArray(parsed)) {
      this.errorChange.emit(t('content.relationExpectedOne', { field: this.field.name }));
      return;
    }
    const entries = Array.isArray(parsed) ? parsed : [parsed];
    const problem = this.relationProblem(entries);
    this.errorChange.emit(problem);
    if (problem === null) {
      this.emitRefs(relationRefsOf(parsed as FieldValue));
    }
  }

  /**
   * The wording the JSON box carries: which key, and what to fill in.
   *
   * One sentence for one reference and one for a list: the shape is what the box has to be filled
   * with, and a page (which has no id) is a different sentence again.
   */
  relationHint(): { key: string; params: Record<string, unknown> } | null {
    if (this.multiple) {
      return { key: 'content.relationJsonHintMany', params: { targets: this.targetNames() } };
    }
    const target = this.targets[0];
    if (target === undefined) {
      return null;
    }
    return {
      key:
        target.kind === 'single_page'
          ? 'content.relationJsonHintPage'
          : 'content.relationJsonHintOne',
      params: { target: target.name },
    };
  }

  /** The targets this field may name, as one line for a message. */
  private targetNames(): string {
    return this.targets.map((target) => target.name).join(', ');
  }

  /**
   * What is wrong with a relation's references, or null when the server would take them.
   *
   * The same rules the server applies (`FieldValue::from_untyped` and the schema save): every
   * reference names one of this field's targets, a collection reference carries the id of an item,
   * and a page reference carries no id. Saying so here means the reader is told in their own
   * language while still looking at the box, rather than by a 400 after the whole form has been
   * sent.
   */
  private relationProblem(entries: unknown[]): Message | null {
    for (const [index, entry] of entries.entries()) {
      const field = `${this.field.name}[${index}]`;
      if (entry === null || typeof entry !== 'object' || Array.isArray(entry)) {
        return t('content.relationShape', { field });
      }
      const record = entry as { target?: unknown; item?: unknown };
      if (typeof record.target !== 'string') {
        return t('content.relationShape', { field });
      }
      // A name may be a collection and a page at once, and the value says which by holding an item
      // (or not), so the shape is what decides which declaration the reference belongs to.
      const named = this.targets.filter((target) => target.name === record.target);
      if (named.length === 0) {
        return t('content.relationTargetMismatch', { field, targets: this.targetNames() });
      }
      if (record.item === undefined || record.item === null) {
        if (!named.some((target) => target.kind === 'single_page')) {
          return t('content.relationItemId', { field });
        }
        continue;
      }
      if (!named.some((target) => target.kind === 'collection')) {
        return t('content.relationPageHasNoItem', { field });
      }
      const item = record.item;
      if (typeof item !== 'number' || !Number.isInteger(item) || item < 1) {
        return t('content.relationItemId', { field });
      }
    }
    return null;
  }

  /**
   * Ask what this field's references are called.
   *
   * Of the value as it is now, not of what was loaded: an editor who has just typed a reference
   * should see it named, not wait for the next save.
   */
  private loadReferenceNames(value: FieldValue) {
    const references = relationRefsOf(value);
    if (references.length === 0) {
      this.referenceNames.set([]);
      return;
    }
    this.relationLabels
      .labelsFor(references)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (labels: Map<string, string>) => {
          this.labels.set(labels);
          // Only what the target's schema names is worth a hint: the box already says the rest (and
          // a chip says the reference itself when there is no name).
          const named = references
            .map((reference) => referenceName(reference, labels))
            .filter((name, index) => labels.has(referenceKey(references[index])));
          this.referenceNames.set(named);
        },
        // A name that cannot be fetched leaves the box as it was: the references are right there.
        error: () => {
          this.labels.set(new Map());
          this.referenceNames.set([]);
        },
      });
  }

  /** Emit the references in the shape the field holds: a list, or one object or `null`. */
  private emitRefs(refs: RelationRef[]) {
    const value: FieldValue = this.multiple ? refs : (refs[0] ?? null);
    this.value = value;
    this.lastEmitted = value;
    this.valueChange.emit(value);
    this.loadReferenceNames(value);
  }

  private syncJsonBox() {
    // Our own emission comes straight back as `value`; re-seeding then would fight the user's
    // typing.
    if (this.value === this.lastEmitted) {
      return;
    }
    const held = this.value === undefined ? null : this.value;
    this.arrayText = JSON.stringify(held ?? (this.multiple ? [] : null));
  }
}
