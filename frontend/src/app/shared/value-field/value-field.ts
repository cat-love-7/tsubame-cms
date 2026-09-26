import {
  Component,
  EventEmitter,
  Input,
  OnChanges,
  OnInit,
  Output,
  SimpleChanges,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe } from '@jsverse/transloco';

import { ImageField } from 'app/shared/image-field/image-field';
import { MarkdownField } from 'app/shared/markdown-field/markdown-field';
import { Message, t } from 'app/core/i18n/message';
import {
  FieldSchema,
  RelationTarget,
  TextFieldOptions,
  isEnumFieldSchema,
  isMarkdownFieldSchema,
  isRelationArraySchema,
  isSlugFieldSchema,
  isTextFieldSchema,
  relationOptionsOf,
} from 'app/models/schema/fields';
import { SLUG_MAX_LENGTH, isUsableSlug, normaliseSlug } from 'app/models/schema/slug';
import { FieldValue } from 'app/models/values/fields';
import { ContentValue } from 'app/models/values/single-page';
import { ArrayField } from 'app/shared/array-field/array-field';
import { CompositeField } from 'app/shared/composite-field/composite-field';
import { RelationField } from 'app/shared/relation-field/relation-field';

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
  | 'RelationArray'
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
    ArrayField,
    CompositeField,
    RelationField,
    MatTooltipModule,
    FormsModule,
    MatButtonModule,
    MatCheckboxModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSelectModule,
    ImageField,
    MarkdownField,
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
   * Show the value rather than the control that would edit it.
   *
   * For the comparison of what is live against what the form holds. A multi-line box there would
   * hold the height the Schema asked for - which is a minimum for *editing* - and one sentence in a
   * twelve-row box is mostly blank space. In this mode the text is rendered as text, sized to what
   * it holds, and the layout minimum the cells inside a Composite would keep is dropped too.
   */
  @Input() compact = false;
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

  ngOnInit() {
    // What was loaded may already be outside the lengths the schema sets: the schema may have
    // been tightened since it was written, so the reader is told before they save it back.
    this.reportTextLength(this.value);
  }

  ngOnChanges(changes: SimpleChanges) {
    if (changes['value']) {
      this.reportTextLength(this.value);
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
    // Several references are an array of relations, and they are edited with the chips of one
    // relation rather than as JSON: the schema says the widgets, and the shape says the value.
    if ('Array' in type) return isRelationArraySchema(type) ? 'RelationArray' : 'Array';
    if ('CompositeField' in type) return 'CompositeField';
    if ('Relation' in type) return 'Relation';
    return 'Unknown';
  }

  /** What a relation value may name: one target, or one per item type of an array of relations. */
  relationTargets(): RelationTarget[] {
    return relationOptionsOf(this.field.field_type).map((options) => options.target);
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
    this.valueChange.emit(value);
    this.reportTextLength(value);
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
}
