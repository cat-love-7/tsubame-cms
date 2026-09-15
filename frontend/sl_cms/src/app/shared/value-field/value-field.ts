import { NgTemplateOutlet } from '@angular/common';
import { Component, EventEmitter, Input, OnChanges, OnInit, Output, SimpleChanges, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { TranslocoPipe } from '@jsverse/transloco';
import { concatMap, from, toArray } from 'rxjs';

import { apiUrl } from 'app/core/api-url';
import { fieldCellStyle } from 'app/core/field-layout';
import { Message, failure, t } from 'app/core/i18n/message';
import {
  FieldSchema,
  TextFieldOptions,
  isArrayFieldSchema,
  isCompositeFieldSchema,
  isEnumFieldSchema,
  isMarkdownFieldSchema,
  isTextFieldSchema,
} from 'app/models/schema/fields';
import { ContentValue } from 'app/models/values/collection';
import { FieldValue, imageIdOf, withDefaults } from 'app/models/values/fields';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { CompositeFieldsService } from 'app/services/schema/composite_fields.service';
import { ImagesService } from 'app/services/media/images.service';

type FieldKind =
  | 'Text'
  | 'Markdown'
  | 'Number'
  | 'Boolean'
  | 'Date'
  | 'DateTime'
  | 'Image'
  | 'TextEnum'
  | 'Array'
  | 'CompositeField'
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
@Component({
  selector: 'app-value-field',
  imports: [
    FormsModule,
    MatButtonModule,
    MatCheckboxModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSelectModule,
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
  @Output() valueChange = new EventEmitter<FieldValue>();
  /**
   * Non-null while the field holds input that cannot be turned into a value (an array
   * edited as JSON that does not parse, a failed upload, or a problem in a sub-field).
   * The parent refuses to save while any field reports one, so bad input is never quietly
   * dropped or replaced by a stale value.
   */
  @Output() errorChange = new EventEmitter<Message | null>();

  private images = inject(ImagesService);
  private compositeFields = inject(CompositeFieldsService);

  public uploading = signal(false);
  /** JSON buffer for Array fields, which are edited as raw JSON. */
  public arrayText = '';
  public imageUrl = apiUrl;
  /** Places a composite's sub-fields in the same grid the top level uses. */
  public cellStyle = fieldCellStyle;

  /** Images already uploaded, so one can be reused instead of uploaded again. */
  public library = signal<ImageEntry[]>([]);
  public pickerOpen = signal(false);
  /** True when the open picker collects several images (for an image array). */
  public pickerMulti = signal(false);
  /** Ids ticked in the multi-image picker. */
  public selected = signal<number[]>([]);
  /** The library is fetched when the picker is first opened, and not before. */
  private libraryLoaded = false;
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
      this.syncArrayBuffer();
      this.syncCompositeValues();
      this.syncElements();
    }
  }

  /** Which input to render. */
  kind(): FieldKind {
    const type = this.field.field_type;
    if (typeof type === 'string') {
      return type as FieldKind;
    }
    if ('Text' in type) return 'Text';
    if ('Markdown' in type) return 'Markdown';
    if ('TextEnum' in type) return 'TextEnum';
    if ('Array' in type) return 'Array';
    if ('CompositeField' in type) return 'CompositeField';
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

  imageId(): number | null {
    return imageIdOf(this.value);
  }

  imagePreviewUrl(): string {
    const value = this.value;
    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
      const url = (value as { url?: unknown }).url;
      if (typeof url === 'string') {
        return url;
      }
    }
    return '';
  }

  update(value: FieldValue) {
    this.value = value;
    this.lastEmitted = value;
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
    const max = this.maxLength();
    const min = this.minLength();
    // Nothing to say about a field the schema left unlimited, and nothing to clear either.
    if (this.textOptions() === null || typeof value !== 'string' || (max === null && min === null)) {
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
    try {
      const parsed = JSON.parse(trimmed);
      if (!Array.isArray(parsed)) {
        this.errorChange.emit(t('content.expectedJsonArray', { field: this.field.name }));
        return;
      }
      this.errorChange.emit(null);
      this.update(parsed as FieldValue);
    } catch {
      this.errorChange.emit(t('content.invalidJson', { field: this.field.name }));
    }
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

  onFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) {
      return;
    }
    this.uploading.set(true);
    this.errorChange.emit(null);
    this.images.uploadImage(file).subscribe({
      next: (info) => {
        this.uploading.set(false);
        // Mirror the shape the API returns for an image value.
        // The stable URL, which the server knows and the client cannot derive: on AWS the
        // upload URL names a signature, not the object's public address.
        this.update({ id: info.id, url: info.url });
        input.value = '';
      },
      error: (e) => {
        this.uploading.set(false);
        this.errorChange.emit(failure('content.uploadFailed', e));
      },
    });
  }

  /** Show the library: `multi` collects several images at once (an image array). */
  openLibrary(multi: boolean) {
    this.pickerMulti.set(multi);
    this.selected.set([]);
    this.pickerOpen.set(true);
    if (this.libraryLoaded) {
      return;
    }
    this.images.listImages().subscribe({
      next: (images) => {
        this.libraryLoaded = true;
        this.library.set(images);
      },
      error: (e) => this.errorChange.emit(failure('content.failedToLoadImages', e)),
    });
  }

  closePicker() {
    this.pickerOpen.set(false);
    this.selected.set([]);
  }

  /** A thumbnail click: single mode uses it, multi mode ticks it. */
  onThumbnail(image: ImageEntry) {
    if (!this.pickerMulti()) {
      this.chooseImage(image);
      return;
    }
    this.selected.update((ids) =>
      ids.includes(image.id) ? ids.filter((id) => id !== image.id) : [...ids, image.id],
    );
  }

  isSelected(id: number): boolean {
    return this.selected().includes(id);
  }

  /** Use a library image; the value keeps the same shape an upload produces. */
  chooseImage(image: ImageEntry) {
    this.update({ id: image.id, url: image.url });
    this.closePicker();
    this.errorChange.emit(null);
  }

  /** Append the ticked images, keeping the order the library lists them in. */
  addSelectedImages() {
    const chosen = this.library().filter((image) => this.selected().includes(image.id));
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
      isArrayFieldSchema(type) && type.Array.length > 0 && type.Array.every((item) => item === 'Image')
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
    const record = value as ContentValue;
    const declaresValues = schema.some((field) => field.name === 'values');
    const wrapped = record['values'];
    if (
      !declaresValues &&
      wrapped !== null &&
      typeof wrapped === 'object' &&
      !Array.isArray(wrapped)
    ) {
      return wrapped as ContentValue;
    }
    return record;
  }

  private syncArrayBuffer() {
    if (!isArrayFieldSchema(this.field.field_type)) {
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
