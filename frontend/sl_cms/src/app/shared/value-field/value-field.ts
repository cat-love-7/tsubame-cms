import { NgTemplateOutlet } from '@angular/common';
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
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatChipsModule } from '@angular/material/chips';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { concatMap, from, toArray } from 'rxjs';

import { apiUrl } from 'app/core/api-url';
import { ImageField } from 'app/shared/image-field/image-field';
import { LibraryPicker } from 'app/shared/library-picker/library-picker';
import { MarkdownField } from 'app/shared/markdown-field/markdown-field';
import { fieldCellStyle } from 'app/core/field-layout';
import { Message, failure, t } from 'app/core/i18n/message';
import {
  FieldSchema,
  RelationTarget,
  TextFieldOptions,
  isArrayFieldSchema,
  isCompositeFieldSchema,
  isEnumFieldSchema,
  isMarkdownFieldSchema,
  isRelationFieldSchema,
  isSlugFieldSchema,
  isTextFieldSchema,
} from 'app/models/schema/fields';
import { SLUG_MAX_LENGTH, isUsableSlug, normaliseSlug } from 'app/models/schema/slug';
import { ContentValue } from 'app/models/values/single-page';

import {
  FieldValue,
  RelationRef,
  referenceKey,
  referenceName,
  relationRefsOf,
  withDefaults,
} from 'app/models/values/fields';
import { RelationLabelsService } from 'app/services/relations/relation-labels.service';
import { RelationPicker } from 'app/shared/relation-picker/relation-picker';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { CompositeFieldsService } from 'app/services/schema/composite-fields.service';
import { ImagesService } from 'app/services/media/images.service';

type FieldKind =
  | 'Text'
  | 'Slug'
  | 'Markdown'
  | 'Number'
  | 'Boolean'
  | 'Date'
  | 'DateTime'
  | 'Image'
  | 'TextEnum'
  | 'Array'
  | 'CompositeField'
  | 'Relation'
  | 'Unknown';

/**
 * Renders the input for one schema field and edits its value.
 *
 * The schema decides which widget appears, and values are held **without type tags**
 * (the schema is what gives them meaning). Kept as a separate component so the content
 * editor and the schema editor's preview render the same thing from one implementation.
 *
 * A composite field renders its sub-fields by referring back to this same component, so
 * nesting works to any depth. That is also why the server refuses composite reference
 * cycles: this recursion would never terminate.
 */
/** Every widget gets its own id, so a label can name one input and not another. */
let nextValueFieldId = 0;

@Component({
  selector: 'app-value-field',
  imports: [
    RelationPicker,
    MatChipsModule,
    MatTooltipModule,
    FormsModule,
    MatButtonModule,
    MatCheckboxModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSelectModule,
    ImageField,
    LibraryPicker,
    MarkdownField,
    NgTemplateOutlet,
    TranslocoPipe,
    ValueField,
  ],
  templateUrl: './value-field.html',
  styleUrl: './value-field.scss',
})
export class ValueField implements OnInit, OnChanges {
  @Input({ required: true }) field!: FieldSchema;
  @Input() value: FieldValue = null;
  /** Renders the widget read-only, for the schema editor's preview. */
  @Input() disabled = false;
  /**
   * The other fields of the item, by name.
   *
   * A slug may be generated from one of them (its schema says which), and a widget only ever sees
   * its own value - so the editor passes the whole set, and this reads the one it needs.
   */
  @Input() siblings: ContentValue = {};
  @Output() valueChange = new EventEmitter<FieldValue>();
  /**
   * Non-null while the field holds input that cannot be turned into a value (an array
   * edited as JSON that does not parse, a failed upload, or a problem in a sub-field).
   * The parent refuses to save while any field reports one, so bad input is never quietly
   * dropped or replaced by a stale value.
   */
  @Output() errorChange = new EventEmitter<Message | null>();

  /**
   * The id of the field's own label, so every input can point at it.
   *
   * The label is a `span` above the widget rather than a Material `mat-label` inside it, and a
   * screen reader has no way to know that on its own: without this, every box in the form is
   * "edit text" and the checkbox is "Yes". The suffix is per widget because a composite may put
   * the same field name on screen twice.
   */
  public readonly labelId = `value-field-${(nextValueFieldId += 1)}`;

  private images = inject(ImagesService);
  /** The wording of the placeholders and prompts the Markdown buttons put up. */
  private i18n = inject(TranslocoService);
  private compositeFields = inject(CompositeFieldsService);
  private relationLabels = inject(RelationLabelsService);
  private destroyRef = inject(DestroyRef);

  public uploading = signal(false);
  /** JSON buffer for Array fields, which are edited as raw JSON. */
  public arrayText = '';
  public imageUrl = apiUrl;
  /** Places a composite's sub-fields in the same grid the top level uses. */
  public cellStyle = fieldCellStyle;

  public pickerOpen = signal(false);
  /** An image array is shown as thumbnails unless the JSON view is asked for. */
  public jsonMode = signal(false);

  /**
   * The elements of a composite array, each with the definition it names and the value the
   * child editor reads. Rebuilt when a value arrives from outside; kept as it is while the
   * user edits, so a keystroke in one element cannot reset another.
   */
  public elementItems: { id: string; value: FieldValue }[] = [];
  /** The schema each element editor needs, one per element and stable between rebuilds. */
  public elementSchemas: FieldSchema[] = [];
  /** The definition a new element uses, when the array allows more than one. */
  public newElementType = signal('');
  /** The composite definitions, for the defaults of a new element. */
  private definitions = new Map<string, FieldSchema[]>();
  private definitionsLoaded = false;

  /** The referenced composite's sub-schema, or null when it is not defined. */
  public compositeSchema = signal<FieldSchema[] | null>(null);
  public compositeId = signal('');
  public compositeValues: ContentValue = {};

  /** The last value this component emitted, so its own output is not mistaken for new
   * input (which would reset the JSON buffer or the sub-field state mid-typing). */
  private lastEmitted: FieldValue = null;

  /** Problems reported by sub-fields, so one clearing does not clear another's. */
  private subErrors: { [field: string]: Message } = {};

  ngOnInit() {
    this.loadReferenceNames(this.value);
    this.syncArrayBuffer();
    this.loadDefinitions();
    this.syncElements();
    this.reportTextLength(this.value);
  }

  ngOnChanges(changes: SimpleChanges) {
    if (changes['field']) {
      this.loadCompositeSchema();
      this.loadDefinitions();
      this.syncElements();
    } else if (changes['value']) {
      this.reportTextLength(this.value);
      this.loadReferenceNames(this.value);
      this.syncArrayBuffer();
      this.syncCompositeValues();
      this.syncElements();
    }
  }

  /** Which input to render. */
  kind(): FieldKind {
    const type = this.field.field_type;
    if (typeof type === 'string') {
      return type;
    }
    if ('Text' in type) return 'Text';
    if ('Slug' in type) return 'Slug';
    if ('Markdown' in type) return 'Markdown';
    if ('TextEnum' in type) return 'TextEnum';
    if ('Array' in type) return 'Array';
    if ('CompositeField' in type) return 'CompositeField';
    if ('Relation' in type) return 'Relation';
    return 'Unknown';
  }

  enumOptions(): string[] {
    return isEnumFieldSchema(this.field.field_type) ? this.field.field_type.TextEnum : [];
  }

  /**
   * The lengths the schema set for this field's text, when it is a text field.
   *
   * Both text kinds carry them, and the schema editor offers them for both, so the widget has to
   * read the same place the server validates.
   */
  private textOptions(): TextFieldOptions | null {
    const type = this.field.field_type;
    if (isTextFieldSchema(type)) {
      return type.Text;
    }
    if (isMarkdownFieldSchema(type)) {
      return type.Markdown;
    }
    return null;
  }

  maxLength(): number | null {
    return this.textOptions()?.max_length ?? null;
  }

  minLength(): number | null {
    return this.textOptions()?.min_length ?? null;
  }

  /** Whether this field is written in a box rather than on one line. */
  multiline(): boolean {
    return isMarkdownFieldSchema(this.field.field_type)
      ? true
      : (this.textOptions()?.multiline ?? false);
  }

  /**
   * How many lines a multi-line text box shows, from the height the schema asked for.
   *
   * `height` is a layout minimum in row units of 72px (see `field-layout`), and a line of text is
   * about 24px in this theme - so one unit is three lines, and a field the schema editor drew four
   * units tall is a box of twelve. The floor is what a text box was before the height meant
   * anything: never a two-line one. (Markdown has its own, in `MarkdownField`.)
   */
  rows(): number {
    return Math.max(3, Math.max(1, this.field.height) * 3);
  }

  /**
   * The item types the schema declared, named the way the schema editor names them.
   *
   * A plain kind is its own name; a text kind carries options and is named by its kind; a
   * composite is named by the definition it holds. Without this the JSON box says nothing about
   * what it accepts, which is the one thing the reader needs to know.
   */
  itemTypeNames(): string[] {
    const type = this.field.field_type;
    if (!isArrayFieldSchema(type)) {
      return [];
    }
    return type.Array.map((item) => {
      if (typeof item === 'string') {
        return item;
      }
      if (isTextFieldSchema(item)) {
        return 'Text';
      }
      if (isMarkdownFieldSchema(item)) {
        return 'Markdown';
      }
      if (isEnumFieldSchema(item)) {
        return 'TextEnum';
      }
      if (isCompositeFieldSchema(item)) {
        return String(item.CompositeField.id);
      }
      return 'Unknown';
    });
  }

  /**
   * Whether a parsed item could be one of the declared item types.
   *
   * Deliberately permissive: it only rules out what *no* declared type could read - a string in
   * an array of numbers, an object in an array of texts. The server tries each declared type in
   * turn and its idea of "parses" is narrower than anything worth copying here, and refusing a
   * value it would have taken is worse than letting it answer. What this buys is the obvious
   * mistake being caught while the reader is still looking at the box.
   */
  private itemFitsDeclaredTypes(item: unknown): boolean {
    const type = this.field.field_type;
    if (!isArrayFieldSchema(type)) {
      return true;
    }
    return type.Array.some((candidate) => {
      if (typeof candidate === 'string') {
        switch (candidate) {
          case 'Number':
            return typeof item === 'number';
          case 'Boolean':
            return typeof item === 'boolean';
          case 'Image':
            // An id, a response object, or nothing chosen.
            return (
              item === null ||
              typeof item === 'number' ||
              (typeof item === 'object' && item !== null && 'id' in item)
            );
          case 'Date':
          case 'DateTime':
            // Both travel as strings; a number is accepted here only because the server may.
            return typeof item === 'string' || typeof item === 'number';
          default:
            return true;
        }
      }
      if (isCompositeFieldSchema(candidate)) {
        return typeof item === 'object' && item !== null;
      }
      // Text, Markdown and enums are all strings on the wire.
      return typeof item === 'string';
    });
  }

  /** Whether the value has to be the only one of its kind in the collection. */
  isUniqueField(): boolean {
    return this.field.unique || this.kind() === 'Slug';
  }

  /** The field a slug is offered to be generated from, when the schema names one. */
  slugSource(): string | null {
    const type = this.field.field_type;
    if (!isSlugFieldSchema(type)) {
      return null;
    }
    const source = type.Slug.generate_from?.trim();
    return source ? source : null;
  }

  /** Whether there is something to generate from, and it would change the value. */
  canGenerateSlug(): boolean {
    const source = this.slugSource();
    if (source === null || this.disabled) {
      return false;
    }
    const value = this.siblings[source];
    return typeof value === 'string' && value !== '' && normaliseSlug(value) !== this.value;
  }

  /**
   * Take the slug from the field the schema names.
   *
   * A button rather than something automatic: a title that changes should not silently move a URL
   * that is already published, so the editor decides when the address moves with it.
   */
  generateSlug() {
    const source = this.slugSource();
    const value = source === null ? undefined : this.siblings[source];
    if (typeof value !== 'string') {
      return;
    }
    this.update(normaliseSlug(value));
  }

  /**
   * Rewrite what was typed into the canonical form, on the way out of the input.
   *
   * The server does this when it stores the value; doing it here as well means the form shows what
   * will be stored, rather than one thing until the save and another after it.
   */
  canonicaliseSlug() {
    if (this.kind() !== 'Slug' || typeof this.value !== 'string' || this.value === '') {
      return;
    }
    const canonical = normaliseSlug(this.value);
    if (canonical !== this.value) {
      this.update(canonical);
    }
  }

  /** How much has been typed, in the same characters the server counts. */
  textLength(): number {
    return typeof this.value === 'string' ? [...this.value].length : 0;
  }

  /** Which catalog line states the limits, or `null` when the field has none. */
  lengthHintKey(): string | null {
    const max = this.maxLength();
    const min = this.minLength();
    if (min !== null && max !== null) {
      return 'content.lengthBetween';
    }
    if (max !== null) {
      return 'content.lengthAtMost';
    }
    return min !== null ? 'content.lengthAtLeast' : null;
  }

  update(value: FieldValue) {
    this.value = value;
    this.lastEmitted = value;
    this.valueChange.emit(value);
    this.reportTextLength(value);
    this.loadReferenceNames(value);
  }

  /**
   * Say so, before the save, when a text value is outside the lengths the schema set.
   *
   * The server answers the same refusal (`field_too_long` / `field_too_short`), but only after the
   * whole form has been sent; reporting it here blocks the save and marks the input instead, and
   * does it in the reader's language. The limits count characters, as the server counts them.
   */
  private reportTextLength(value: FieldValue) {
    if (this.kind() === 'Slug') {
      this.reportSlug(value);
      return;
    }
    const max = this.maxLength();
    const min = this.minLength();
    // Nothing to say about a field the schema left unlimited, and nothing to clear either.
    if (
      this.textOptions() === null ||
      typeof value !== 'string' ||
      (max === null && min === null)
    ) {
      return;
    }
    if (max !== null && [...value].length > max) {
      this.errorChange.emit(t('content.valueTooLong', { field: this.field.name, max }));
      return;
    }
    if (min !== null && value.length > 0 && [...value].length < min) {
      this.errorChange.emit(t('content.valueTooShort', { field: this.field.name, min }));
      return;
    }
    this.errorChange.emit(null);
  }

  /**
   * Say so, before the save, when a value cannot become a slug.
   *
   * The same two answers the server gives (`invalid_slug`, `field_too_long`), for the same reason
   * the text lengths are checked here: a refusal after the whole form has been sent is a worse way
   * to learn it.
   */
  private reportSlug(value: FieldValue) {
    if (typeof value !== 'string') {
      return;
    }
    if (value !== '' && !isUsableSlug(value)) {
      this.errorChange.emit(t('errors.invalid_slug', { field: this.field.name }));
      return;
    }
    if (normaliseSlug(value).length > SLUG_MAX_LENGTH) {
      this.errorChange.emit(
        t('content.valueTooLong', { field: this.field.name, max: SLUG_MAX_LENGTH }),
      );
      return;
    }
    this.errorChange.emit(null);
  }

  /** `datetime-local` inputs want local wall time without a timezone. */
  toDateTimeLocal(): string {
    if (typeof this.value !== 'string' || this.value === '') {
      return '';
    }
    const date = new Date(this.value);
    if (Number.isNaN(date.getTime())) {
      return '';
    }
    const pad = (n: number) => String(n).padStart(2, '0');
    return (
      `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
      `T${pad(date.getHours())}:${pad(date.getMinutes())}`
    );
  }

  setDateTime(local: string) {
    if (!local) {
      this.update(null);
      return;
    }
    const date = new Date(local);
    this.update(Number.isNaN(date.getTime()) ? null : date.toISOString());
  }

  onArrayTextChange(text: string) {
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
    // A relation is a set of references, so it is array-shaped too - but what an element may hold
    // is what its own target says, not the array item types.
    if (isRelationFieldSchema(this.field.field_type)) {
      const problem = this.relationProblem(parsed);
      this.errorChange.emit(problem);
      if (problem === null) {
        this.update(parsed as FieldValue);
      }
      return;
    }
    // The declared item types are what the array accepts, so an item none of them could read is
    // reported here rather than by the server after the whole form has been sent.
    const unsuitable = parsed.findIndex((item) => !this.itemFitsDeclaredTypes(item));
    if (unsuitable >= 0) {
      this.errorChange.emit(
        t('content.arrayItemType', {
          field: `${this.field.name}[${unsuitable}]`,
          types: this.itemTypeNames().join(', '),
        }),
      );
      return;
    }
    this.errorChange.emit(null);
    this.update(parsed as FieldValue);
  }

  /**
   * The names of the references this field holds, when the target's schema names them.
   *
   * The box edits the references themselves, so this is what tells an editor what they just wrote:
   * `categories #3` is a reference, and `技術` is the item it points at.
   */
  public referenceNames = signal<string[]>([]);

  /** Whether the reference picker is open under this field. */
  public relationPickerOpen = signal(false);

  /** What this relation points at, as the picker wants it. */
  public relationTarget(): RelationTarget | null {
    const type = this.field.field_type;
    return isRelationFieldSchema(type) ? type.Relation.target : null;
  }

  /** The labels the target's schema answered, by reference. */
  private labels = signal<ReadonlyMap<string, string>>(new Map());

  /**
   * The references this field holds, as a set.
   *
   * Read from the value rather than kept beside it: the chips, the picker and the JSON box all edit
   * the one value, and a second copy is how the two drift apart.
   */
  public relationRefs(): RelationRef[] {
    return relationRefsOf(this.value);
  }

  /** Whether this field holds one reference, which is what a pick replaces rather than adds to. */
  public relationIsSingle(): boolean {
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
  public toggleReference(reference: RelationRef) {
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
  public moveReference(index: number, delta: number) {
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
  public referenceLabel(reference: RelationRef): string {
    return referenceName(reference, this.labels());
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

  /**
   * The wording a relation's JSON box carries: which key, and what to fill in.
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

  setCompositeValue(subField: FieldSchema, value: FieldValue) {
    this.compositeValues[subField.name] = value;
    // Emit the bare object of sub-values: that is what a composite write accepts. The
    // `{id, values}` wrapper only appears on reads.
    this.update({ ...this.compositeValues });
  }

  forwardCompositeError(subField: FieldSchema, problem: Message | null) {
    // One at a time: the parent shows the first problem and refuses to save until none remain,
    // so naming the others too would only lengthen the message.
    this.reportSubError(subField.name, problem);
  }

  /** Show the library, to add images to an image array. */
  openLibrary() {
    this.pickerOpen.set(true);
  }

  closePicker() {
    this.pickerOpen.set(false);
  }

  /** Append the images the picker confirmed, in the order it listed them. */
  addChosenImages(chosen: ImageEntry[]) {
    const images = chosen.map((image) => ({ id: image.id, url: image.url }));
    this.updateArray([...this.arrayItems(), ...images]);
    this.closePicker();
    this.errorChange.emit(null);
  }

  /**
   * Upload files straight into an image array, without a detour through the library.
   *
   * They are uploaded one after another so the array keeps the order the files were
   * chosen in. A failure part-way appends nothing (the files that did upload stay in the
   * library, so they can be picked from there); retrying is the editor's decision.
   */
  onFilesSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    if (files.length === 0) {
      return;
    }
    this.uploading.set(true);
    this.errorChange.emit(null);
    from(files)
      .pipe(
        concatMap((file) => this.images.uploadImage(file)),
        toArray(),
      )
      .subscribe({
        next: (uploaded) => {
          this.uploading.set(false);
          input.value = '';
          this.updateArray([
            ...this.arrayItems(),
            // Mirror the shape an image value has.
            ...uploaded.map((info) => ({ id: info.id, url: info.url })),
          ]);
        },
        error: (e) => {
          this.uploading.set(false);
          input.value = '';
          this.errorChange.emit(failure('content.uploadFailed', e));
        },
      });
  }

  /** The composite definitions this array declares as item types. */
  compositeItemTypes(): string[] {
    const type = this.field.field_type;
    if (!isArrayFieldSchema(type)) {
      return [];
    }
    return type.Array.filter(isCompositeFieldSchema).map((item) => item.CompositeField.id);
  }

  /**
   * A composite array is edited element by element.
   *
   * Only when *every* declared item type is a composite: an element carries no type tag, so a
   * mixed array (say a text and a composite) has nothing that says which editor an element
   * needs. Those keep the JSON view, which does accept composite elements.
   */
  isCompositeArray(): boolean {
    const type = this.field.field_type;
    return (
      isArrayFieldSchema(type) &&
      type.Array.length > 0 &&
      type.Array.every((item) => isCompositeFieldSchema(item))
    );
  }

  /** Add an element of the chosen definition, with that definition's defaults. */
  addElement() {
    const id = this.newElementType() || this.compositeItemTypes()[0];
    if (!id) {
      return;
    }
    const schema = this.definitions.get(id);
    const values = schema ? withDefaults(schema, {}) : {};
    this.setElements([...this.arrayItems(), { id, values }]);
  }

  removeElementAt(index: number) {
    const items = this.arrayItems();
    items.splice(index, 1);
    // The reported problems are keyed by position, so they mean nothing after this.
    this.clearElementErrors();
    this.setElements(items);
  }

  moveElement(index: number, delta: number) {
    const items = this.arrayItems();
    const target = index + delta;
    if (target < 0 || target >= items.length) {
      return;
    }
    [items[index], items[target]] = [items[target], items[index]];
    this.clearElementErrors();
    this.setElements(items);
  }

  /** One element changed: keep the definition it names and store the new sub-values. */
  setElementValues(index: number, values: FieldValue) {
    const items = this.arrayItems();
    const id = this.elementItems[index]?.id ?? this.compositeItemTypes()[0] ?? '';
    items[index] = { id, values };
    this.elementItems[index] = { id, value: { id, values } };
    this.arrayText = JSON.stringify(items);
    this.update(items as FieldValue);
  }

  reportElementError(index: number, problem: Message | null) {
    this.reportSubError(`element:${index}`, problem);
  }

  /** An array of images, which is edited with thumbnails instead of raw JSON. */
  isImageArray(): boolean {
    const type = this.field.field_type;
    return (
      isArrayFieldSchema(type) &&
      type.Array.length > 0 &&
      type.Array.every((item) => item === 'Image')
    );
  }

  /**
   * The current array value as images.
   *
   * A bare id (which the JSON view accepts, since an image id is a number) has no URL to
   * show until the item is saved and read back, so it renders as a placeholder.
   */
  arrayImages(): { id: number | null; url: string }[] {
    return this.arrayItems().map((element) => {
      if (typeof element === 'number') {
        return { id: element, url: '' };
      }
      if (element !== null && typeof element === 'object') {
        const record = element as { id?: unknown; url?: unknown };
        return {
          id: typeof record.id === 'number' ? record.id : null,
          url: typeof record.url === 'string' ? record.url : '',
        };
      }
      return { id: null, url: '' };
    });
  }

  removeImageAt(index: number) {
    const items = this.arrayItems();
    items.splice(index, 1);
    this.updateArray(items);
  }

  moveImage(index: number, delta: number) {
    const items = this.arrayItems();
    const target = index + delta;
    if (target < 0 || target >= items.length) {
      return;
    }
    [items[index], items[target]] = [items[target], items[index]];
    this.updateArray(items);
  }

  private arrayItems(): unknown[] {
    return Array.isArray(this.value) ? [...(this.value as unknown[])] : [];
  }

  /** Replace a composite array's value and keep the JSON view and the element editors in step. */
  private setElements(items: unknown[]) {
    this.elementItems = items.map((item, index) => ({
      id: this.idOf(item, index),
      value: (item ?? null) as FieldValue,
    }));
    this.elementSchemas = this.elementItems.map((element) => this.schemaFor(element));
    this.arrayText = JSON.stringify(items);
    this.update(items as FieldValue);
  }

  /** Rebuild the element editors from `value`, unless they already match it. */
  private syncElements() {
    if (!this.isCompositeArray()) {
      return;
    }
    // The chosen definition for a new element: the first one until the user picks otherwise.
    this.newElementType.update((current) => current || this.compositeItemTypes()[0] || '');
    const items = this.arrayItems();
    const matches =
      this.elementItems.length === items.length &&
      this.elementItems.every((element, index) => element.value === items[index]);
    if (matches) {
      return;
    }
    this.elementItems = items.map((item, index) => ({
      id: this.idOf(item, index),
      value: (item ?? null) as FieldValue,
    }));
    this.elementSchemas = this.elementItems.map((element) => this.schemaFor(element));
  }

  /**
   * The definition an element uses: the one it names, or the first the array declares.
   *
   * A read names it (`{id, values}`); a value written by an older client may be the bare object
   * of sub-values, which leaves the first declared definition as the only reading.
   */
  private idOf(item: unknown, _index: number): string {
    const record = (item ?? {}) as { id?: unknown };
    const declared = typeof record.id === 'string' && record.id ? record.id : '';
    return declared || this.compositeItemTypes()[0] || '';
  }

  /** The schema an element editor needs: the composite it names, at the array's height. */
  private schemaFor(element: { id: string; value: FieldValue }): FieldSchema {
    return {
      name: `${this.field.name}[${element.id}]`,
      field_type: { CompositeField: { id: element.id } },
      required: false,
      width: 12,
      height: this.field.height || 1,
    };
  }

  /** The definitions an element may use, for a new element's defaults. */
  private loadDefinitions() {
    if (this.definitionsLoaded || this.compositeItemTypes().length === 0) {
      return;
    }
    // The service caches the list, so a form with several composite arrays asks for it once.
    this.compositeFields.getAllCompositeFields().subscribe({
      next: (all) => {
        this.definitions = new Map(Object.entries(all));
        this.definitionsLoaded = true;
      },
      // An element editor loads the definition it needs and reports it if it is missing, so a
      // failure here is not a reason to hide values that are already stored.
      error: () => {},
    });
  }

  /** One place for the problems the sub-editors report, so one clearing does not clear another. */
  private reportSubError(key: string, problem: Message | null) {
    if (problem) {
      this.subErrors[key] = problem;
    } else {
      delete this.subErrors[key];
    }
    this.errorChange.emit(Object.values(this.subErrors)[0] ?? null);
  }

  private clearElementErrors() {
    for (const key of Object.keys(this.subErrors)) {
      if (key.startsWith('element:')) {
        delete this.subErrors[key];
      }
    }
    this.errorChange.emit(Object.values(this.subErrors)[0] ?? null);
  }

  /** Replace the array value and keep the JSON view in step with it. */
  private updateArray(items: unknown[]) {
    this.arrayText = JSON.stringify(items);
    this.update(items as FieldValue);
  }

  private loadCompositeSchema() {
    const type = this.field.field_type;
    if (!isCompositeFieldSchema(type)) {
      return;
    }
    const id = String(type.CompositeField.id);
    this.compositeId.set(id);
    this.compositeFields.getAllCompositeFields().subscribe({
      next: (all) => {
        const schema = all[id] ?? null;
        this.compositeSchema.set(schema);
        this.compositeValues = schema ? withDefaults(schema, this.innerCompositeValue(schema)) : {};
      },
      error: (e) => this.errorChange.emit(failure('content.failedToLoadComposite', e, { id })),
    });
  }

  private syncCompositeValues() {
    const schema = this.compositeSchema();
    if (!schema || this.value === this.lastEmitted) {
      return;
    }
    this.compositeValues = withDefaults(schema, this.innerCompositeValue(schema));
  }

  /**
   * A composite's own sub-values.
   *
   * Reads wrap them as `{id, values}`, so the wrapper is unwrapped here — except when the
   * composite genuinely declares a sub-field called `values`, which mirrors what the
   * server does.
   */
  private innerCompositeValue(schema: FieldSchema[]): ContentValue {
    const value = this.value;
    if (value === null || typeof value !== 'object' || Array.isArray(value)) {
      return {};
    }
    const record = value;
    const declaresValues = schema.some((field) => field.name === 'values');
    const wrapped = record['values'];
    if (
      !declaresValues &&
      wrapped !== null &&
      typeof wrapped === 'object' &&
      !Array.isArray(wrapped)
    ) {
      return wrapped;
    }
    return record;
  }

  private syncArrayBuffer() {
    const type = this.field.field_type;
    if (!isArrayFieldSchema(type) && !isRelationFieldSchema(type)) {
      return;
    }
    // Our own emission comes straight back as `value`; re-seeding then would fight the
    // user's typing.
    if (this.value === this.lastEmitted) {
      return;
    }
    this.arrayText = JSON.stringify(this.value ?? []);
  }
}
