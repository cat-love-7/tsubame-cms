import {
  Component,
  EventEmitter,
  Input,
  OnChanges,
  OnInit,
  Output,
  SimpleChanges,
  TemplateRef,
  inject,
  signal,
} from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatDialog } from '@angular/material/dialog';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe } from '@jsverse/transloco';
import { concatMap, from, toArray } from 'rxjs';

import { apiUrl } from 'app/core/api-url';
import { Message, failure, t } from 'app/core/i18n/message';
import {
  FieldSchema,
  isArrayFieldSchema,
  isCompositeFieldSchema,
  isEnumFieldSchema,
  isMarkdownFieldSchema,
  isRelationFieldSchema,
  isTextFieldSchema,
} from 'app/models/schema/fields';
import { FieldValue, withDefaults } from 'app/models/values/fields';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { ImagesService } from 'app/services/media/images.service';
import { CompositeFieldsService } from 'app/services/schema/composite-fields.service';
import { openImagePickerMany } from 'app/shared/library-picker/library-picker';
import { ProblemCollector } from 'app/shared/value-field/problem-collector';

/**
 * What one element editor of a composite array needs.
 *
 * The widget owns the list, the add and the remove; what an element *is* edited with belongs to
 * the field above, which is the component that knows how to render any field - and which this
 * widget cannot name, since that component renders this one for an array. So it is handed a
 * template, and the context carries the callbacks the element editor binds to.
 */
export interface ArrayElementContext {
  /** The schema of the definition this element names. */
  schema: FieldSchema;
  /** What the element holds now. */
  value: FieldValue;
  /** An element editor emits the sub-values it edited. */
  changed: (value: FieldValue) => void;
  /** An element editor reports the problem it holds, or null. */
  problem: (problem: Message | null) => void;
}

/**
 * An array value: images as thumbnails, composite elements as element editors, and everything
 * else - along with what the thumbnails and the elements cannot express - as JSON.
 *
 * An array is the one field kind whose elements are edited by other fields, so this widget's
 * element editors arrive as a template: `ValueField` renders this widget and passes it a
 * template that renders a `ValueField`. That is how a composite array nests to any depth
 * without the two components importing each other.
 */
@Component({
  selector: 'app-array-field',
  imports: [
    NgTemplateOutlet,
    FormsModule,
    MatButtonModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSelectModule,
    MatTooltipModule,
    TranslocoPipe,
  ],
  templateUrl: './array-field.html',
  styleUrl: './array-field.scss',
})
export class ArrayField implements OnInit, OnChanges {
  private images = inject(ImagesService);
  private compositeFields = inject(CompositeFieldsService);
  private dialog = inject(MatDialog);

  @Input({ required: true }) field!: FieldSchema;
  @Input() value: FieldValue = null;
  /** Renders the controls away, for the schema editor's preview. */
  @Input() disabled = false;
  @Input() labelId = '';
  /** How one element of a composite array is edited; see [`ArrayElementContext`]. */
  @Input({ required: true }) elementTemplate!: TemplateRef<ArrayElementContext>;
  @Output() valueChange = new EventEmitter<FieldValue>();
  @Output() errorChange = new EventEmitter<Message | null>();

  /** The JSON box's text, which is both the fallback view and what a composite array writes. */
  public arrayText = '';
  /** An image array is shown as thumbnails unless the JSON view is asked for. */
  public jsonMode = signal(false);
  public uploading = signal(false);
  public imageUrl = apiUrl;

  /**
   * The elements of a composite array, each with the definition it names and the value the child
   * editor reads. Rebuilt when a value arrives from outside; kept as it is while the user edits,
   * so a keystroke in one element cannot reset another.
   */
  public elementItems: { id: string; value: FieldValue }[] = [];
  /** The schema each element editor needs, one per element and stable between rebuilds. */
  public elementSchemas: FieldSchema[] = [];
  /** The definition a new element uses, when the array allows more than one. */
  public newElementType = signal('');
  /** The composite definitions, for the defaults of a new element. */
  private definitions = new Map<string, FieldSchema[]>();
  private definitionsLoaded = false;

  /** The last value this component emitted, so its own output is not mistaken for new input
   * (which would reset the JSON box mid-typing). */
  private lastEmitted: FieldValue = null;

  /** Problems reported by the element editors, so one clearing does not clear another's. */
  private problems = new ProblemCollector((problem) => this.errorChange.emit(problem));

  ngOnInit() {
    this.syncJsonBox();
    this.loadDefinitions();
    this.syncElements();
  }

  ngOnChanges(changes: SimpleChanges) {
    if (changes['field']) {
      this.loadDefinitions();
      this.syncElements();
    } else if (changes['value']) {
      this.syncJsonBox();
      this.syncElements();
    }
  }

  /**
   * The names of the item types the schema declares.
   *
   * What the JSON box says under it: the reader of an array edited as JSON is looking at raw
   * values, so telling them which item types the schema declares is telling them what it accepts,
   * which is the one thing the reader needs to know.
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
      if (isRelationFieldSchema(item)) {
        return 'Relation';
      }
      return 'Unknown';
    });
  }

  /**
   * Take the array the JSON box holds.
   *
   * The declared item types are what the array accepts, so an item none of them could read is
   * reported here rather than by the server after the whole form has been sent.
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
      if (isRelationFieldSchema(candidate)) {
        // A reference object; a mixed array's relations are edited as JSON, chips and all only
        // when every item type is a relation (see `ValueField`).
        return typeof item === 'object' && item !== null;
      }
      // Text, Markdown and enums are all strings on the wire.
      return typeof item === 'string';
    });
  }

  /** Show the library, to add images to an image array. */
  openLibrary() {
    openImagePickerMany(this.dialog).subscribe((chosen) => {
      if (chosen) {
        this.addChosenImages(chosen);
      }
    });
  }

  /** Append the images the picker confirmed, in the order it listed them. */
  addChosenImages(chosen: ImageEntry[]) {
    const images = chosen.map((image) => ({ id: image.id, url: image.url }));
    this.updateArray([...this.arrayItems(), ...images]);
    this.errorChange.emit(null);
  }

  /**
   * Upload files straight into an image array, without a detour through the library.
   *
   * They are uploaded one after another so the array keeps the order the files were chosen in. A
   * failure part-way appends nothing (the files that did upload stay in the library, so they can
   * be picked from there); retrying is the editor's decision.
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

  /** The context one element editor is rendered with. */
  elementContext(index: number): ArrayElementContext {
    return {
      schema: this.elementSchemas[index],
      value: this.elementItems[index]?.value ?? null,
      changed: (value: FieldValue) => this.setElementValues(index, value),
      problem: (problem: Message | null) => this.reportElementError(index, problem),
    };
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
    this.problems.clear('element:');
    this.setElements(items);
  }

  moveElement(index: number, delta: number) {
    const items = this.arrayItems();
    const target = index + delta;
    if (target < 0 || target >= items.length) {
      return;
    }
    [items[index], items[target]] = [items[target], items[index]];
    this.problems.clear('element:');
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
    this.problems.set(`element:${index}`, problem);
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

  /** Replace the array value and keep the JSON view in step with it. */
  private updateArray(items: unknown[]) {
    this.arrayText = JSON.stringify(items);
    this.update(items as FieldValue);
  }

  private update(value: FieldValue) {
    this.lastEmitted = value;
    this.valueChange.emit(value);
  }

  /** Seed or re-seed the JSON box from the value, unless the value is our own emission. */
  private syncJsonBox() {
    // Our own emission comes straight back as `value`; re-seeding then would fight the user's
    // typing.
    if (this.value === this.lastEmitted) {
      return;
    }
    this.arrayText = JSON.stringify(this.value ?? []);
  }
}
