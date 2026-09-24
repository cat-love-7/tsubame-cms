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
import { FieldSchema, RelationTarget, isRelationFieldSchema } from 'app/models/schema/fields';
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
 * The chips are the value, and the JSON box (which is what the API takes) stays behind a toggle for
 * a value the picker cannot express. The value is a set of references - `{target, item}` for an
 * item, `{target}` for a page - so every control here edits the same list, and the names come from
 * the target's own schema rather than from this client.
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
  private lastEmitted: FieldValue = null;

  ngOnInit() {
    this.loadReferenceNames(this.value);
    this.syncJsonBox();
  }

  ngOnChanges(changes: SimpleChanges) {
    if (changes['value'] || changes['field']) {
      this.loadReferenceNames(this.value);
      this.syncJsonBox();
    }
  }

  /** What this relation points at, as the picker wants it. */
  relationTarget(): RelationTarget | null {
    const type = this.field.field_type;
    return isRelationFieldSchema(type) ? type.Relation.target : null;
  }

  /**
   * The references this field holds, as a set.
   *
   * Read from the value rather than kept beside it: the chips, the picker and the JSON box all edit
   * the one value, and a second copy is how the two drift apart.
   */
  relationRefs(): RelationRef[] {
    return relationRefsOf(this.value);
  }

  /** Whether this field holds one reference, which is what a pick replaces rather than adds to. */
  relationIsSingle(): boolean {
    const type = this.field.field_type;
    if (!isRelationFieldSchema(type)) {
      return true;
    }
    return type.Relation.target.kind === 'single_page' || !type.Relation.has_many;
  }

  /**
   * Add the reference the picker chose, or take it away when it was already there.
   *
   * A set: the order does not matter, and clicking what is picked is how it is unpicked. A single
   * reference replaces what it held rather than refusing the pick - the picker has already said
   * which one it wants.
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
    this.update(next);
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
    this.update(references);
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
   * against the same rules the server applies, and the value only moves when they pass.
   */
  onJsonChange(text: string) {
    this.arrayText = text;
    const trimmed = text.trim();
    if (trimmed === '') {
      this.errorChange.emit(null);
      this.update([]);
      return;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(trimmed);
    } catch {
      this.errorChange.emit(t('content.invalidJson', { field: this.field.name }));
      return;
    }
    if (!Array.isArray(parsed)) {
      this.errorChange.emit(t('content.expectedJsonArray', { field: this.field.name }));
      return;
    }
    const problem = this.relationProblem(parsed);
    this.errorChange.emit(problem);
    if (problem === null) {
      this.update(parsed as FieldValue);
    }
  }

  /**
   * The wording the JSON box carries: which key, and what to fill in.
   *
   * Three wordings rather than one with flags: whether it holds one or several, and whether the
   * target is a page (which has no id), are each a different sentence.
   */
  relationHint(): { key: string; params: Record<string, unknown> } | null {
    const type = this.field.field_type;
    if (!isRelationFieldSchema(type)) {
      return null;
    }
    const { target, has_many } = type.Relation;
    const key =
      target.kind === 'single_page'
        ? 'content.relationJsonHintPage'
        : has_many
          ? 'content.relationJsonHintMany'
          : 'content.relationJsonHintOne';
    return { key, params: { target: target.name } };
  }

  /**
   * What is wrong with a relation's references, or null when the server would take them.
   *
   * The same rules the server applies (`FieldValue::from_untyped` and the schema save): every
   * reference names this field's target, a collection reference carries the id of an item, a page
   * reference carries no id, and a single reference holds at most one. Saying so here means the
   * reader is told in their own language while still looking at the box, rather than by a 400
   * after the whole form has been sent.
   */
  private relationProblem(refs: unknown[]): Message | null {
    const type = this.field.field_type;
    if (!isRelationFieldSchema(type)) {
      return null;
    }
    const { target, has_many } = type.Relation;
    // A page is one item, and a single reference holds one: the server calls either "one".
    if ((target.kind === 'single_page' || !has_many) && refs.length > 1) {
      return t('content.relationSingle', { field: this.field.name });
    }
    for (const [index, ref] of refs.entries()) {
      const field = `${this.field.name}[${index}]`;
      if (ref === null || typeof ref !== 'object' || Array.isArray(ref)) {
        return t('content.relationShape', { field });
      }
      const record = ref as { target?: unknown; item?: unknown };
      if (record.target !== target.name) {
        return t('content.relationTargetMismatch', { field, target: target.name });
      }
      if (target.kind === 'single_page') {
        if (record.item !== undefined && record.item !== null) {
          return t('content.relationPageHasNoItem', { field });
        }
        continue;
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

  private update(value: FieldValue) {
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
    this.arrayText = JSON.stringify(this.value ?? []);
  }
}
