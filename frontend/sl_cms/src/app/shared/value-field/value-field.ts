import {
  Component,
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
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';

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
    MatCheckboxModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSelectModule,
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
  @Output() errorChange = new EventEmitter<string | null>();

  private images = inject(ImagesService);
  private compositeFields = inject(CompositeFieldsService);

  public uploading = signal(false);
  /** JSON buffer for Array fields, which are edited as raw JSON. */
  public arrayText = '';
  public imageUrl = apiUrl;

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
