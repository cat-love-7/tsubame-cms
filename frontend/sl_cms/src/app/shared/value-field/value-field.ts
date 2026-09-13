import { NgTemplateOutlet } from '@angular/common';
import { Component, EventEmitter, Input, OnChanges, OnInit, Output, SimpleChanges, inject, signal, ChangeDetectionStrategy } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { concatMap, from, toArray } from 'rxjs';

import { apiUrl } from 'app/core/api-url';
import { errorMessage as message } from 'app/core/http-error';
import {
  FieldSchema,
  isArrayFieldSchema,
  isCompositeFieldSchema,
  isEnumFieldSchema,
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
    ValueField,
  ],
  templateUrl: './value-field.html',
  changeDetection: ChangeDetectionStrategy.Eager,
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
  @Output() errorChange = new EventEmitter<string | null>();

  private images = inject(ImagesService);
  private compositeFields = inject(CompositeFieldsService);

  public uploading = signal(false);
  /** JSON buffer for Array fields, which are edited as raw JSON. */
  public arrayText = '';
  public imageUrl = apiUrl;

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

  /** The referenced composite's sub-schema, or null when it is not defined. */
  public compositeSchema = signal<FieldSchema[] | null>(null);
  public compositeId = signal('');
  public compositeValues: ContentValue = {};

  /** The last value this component emitted, so its own output is not mistaken for new
   * input (which would reset the JSON buffer or the sub-field state mid-typing). */
  private lastEmitted: FieldValue = null;

  /** Problems reported by sub-fields, so one clearing does not clear another's. */
  private subErrors: { [field: string]: string } = {};

  ngOnInit() {
    this.syncArrayBuffer();
  }

  ngOnChanges(changes: SimpleChanges) {
    if (changes['field']) {
      this.loadCompositeSchema();
    } else if (changes['value']) {
      this.syncArrayBuffer();
      this.syncCompositeValues();
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
        this.errorChange.emit(`Field '${this.field.name}': expected a JSON array`);
        return;
      }
      this.errorChange.emit(null);
      this.update(parsed as FieldValue);
    } catch {
      this.errorChange.emit(`Field '${this.field.name}': invalid JSON`);
    }
  }

  setCompositeValue(subField: FieldSchema, value: FieldValue) {
    this.compositeValues[subField.name] = value;
    // Emit the bare object of sub-values: that is what a composite write accepts. The
    // `{id, values}` wrapper only appears on reads.
    this.update({ ...this.compositeValues });
  }

  forwardCompositeError(subField: FieldSchema, problem: string | null) {
    if (problem) {
      this.subErrors[subField.name] = problem;
    } else {
      delete this.subErrors[subField.name];
    }
    const remaining = Object.values(this.subErrors);
    this.errorChange.emit(remaining.length > 0 ? remaining.join('; ') : null);
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
        this.update({ id: info.id, url: info.upload_url.split('?')[0] });
        input.value = '';
      },
      error: (e) => {
        this.uploading.set(false);
        this.errorChange.emit(`Upload failed: ${message(e)}`);
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
      error: (e) => this.errorChange.emit(`Failed to load the images: ${message(e)}`),
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
            ...uploaded.map((info) => ({ id: info.id, url: info.upload_url.split('?')[0] })),
          ]);
        },
        error: (e) => {
          this.uploading.set(false);
          input.value = '';
          this.errorChange.emit(`Upload failed: ${message(e)}`);
        },
      });
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
      error: (e) => this.errorChange.emit(`Failed to load composite field '${id}': ${message(e)}`),
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
