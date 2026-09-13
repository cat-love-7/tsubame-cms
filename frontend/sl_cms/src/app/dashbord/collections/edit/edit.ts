import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';

import { apiUrl } from 'app/core/api-url';
import { errorMessage as message } from 'app/core/http-error';
import { CollectionSchema } from 'app/models/schema/collection';
import {
  FieldSchema,
  isArrayFieldSchema,
  isCompositeFieldSchema,
  isEnumFieldSchema,
} from 'app/models/schema/fields';
import { CollectionValue } from 'app/models/values/collection';
import { FieldValue, imageIdOf, withDefaults } from 'app/models/values/fields';
import { ImagesService } from 'app/services/media/images.service';
import { CollectionsService } from 'app/services/schema/collections.service';

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
 * Create/edit one collection item.
 *
 * The form is driven by the collection schema: each field's type decides which input is
 * rendered, and values are sent **without type tags** (the schema is what gives them
 * meaning). Fields this editor cannot edit yet (composite fields) keep whatever the
 * server sent, so saving never silently discards them.
 */
@Component({
  selector: 'app-item-edit',
  imports: [
    FormsModule,
    RouterLink,
    MatButtonModule,
    MatCheckboxModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSelectModule,
  ],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class Edit {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private collectionsService = inject(CollectionsService);
  private imagesService = inject(ImagesService);

  public collectionName: string = this.route.snapshot.params['name'];
  private readonly itemId: number | null =
    this.route.snapshot.params['id'] === undefined
      ? null
      : Number(this.route.snapshot.params['id']);
  public readonly isNew = this.itemId === null;

  public schema: CollectionSchema = [];
  public values: CollectionValue = {};
  public error = signal('');
  public uploading = signal(false);
  /** Exposed for the template. */
  public imageUrl = apiUrl;
  /** JSON buffers for Array fields, which are edited as raw JSON. */
  public arrayText: { [field: string]: string } = {};

  constructor() {
    this.collectionsService.getCollectionSchema(this.collectionName).subscribe({
      next: (schema) => {
        this.schema = schema;
        if (this.itemId === null) {
          this.values = withDefaults(schema, {});
        } else {
          this.loadItem(schema, this.itemId);
        }
      },
      error: (e) => this.error.set(`Failed to load the schema: ${message(e)}`),
    });
  }

  private loadItem(schema: CollectionSchema, id: number) {
    this.collectionsService.getCollectionItem(this.collectionName, id).subscribe({
      next: (values) => {
        this.values = withDefaults(schema, values);
        for (const field of schema) {
          if (isArrayFieldSchema(field.field_type)) {
            this.arrayText[field.name] = JSON.stringify(this.values[field.name] ?? []);
          }
        }
      },
      error: (e) => this.error.set(`Failed to load the item: ${message(e)}`),
    });
  }

  /** Which input to render for `field`. */
  kind(field: FieldSchema): FieldKind {
    const type = field.field_type;
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

  enumOptions(field: FieldSchema): string[] {
    return isEnumFieldSchema(field.field_type) ? field.field_type.TextEnum : [];
  }

  isComposite(field: FieldSchema): boolean {
    return isCompositeFieldSchema(field.field_type);
  }

  imageId(field: FieldSchema): number | null {
    return imageIdOf(this.values[field.name]);
  }

  imagePreviewUrl(field: FieldSchema): string {
    const value = this.values[field.name];
    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
      const url = (value as { url?: unknown }).url;
      if (typeof url === 'string') {
        return url;
      }
    }
    return '';
  }

  /** `datetime-local` inputs want local wall time without a timezone. */
  toDateTimeLocal(field: FieldSchema): string {
    const value = this.values[field.name];
    if (typeof value !== 'string' || value === '') {
      return '';
    }
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) {
      return '';
    }
    const pad = (n: number) => String(n).padStart(2, '0');
    return (
      `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
      `T${pad(date.getHours())}:${pad(date.getMinutes())}`
    );
  }

  setDateTime(field: FieldSchema, local: string) {
    if (!local) {
      this.values[field.name] = null;
      return;
    }
    const date = new Date(local);
    this.values[field.name] = Number.isNaN(date.getTime()) ? null : date.toISOString();
  }

  onFileSelected(field: FieldSchema, event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) {
      return;
    }
    this.uploading.set(true);
    this.error.set('');
    this.imagesService.uploadImage(file).subscribe({
      next: (info) => {
        this.uploading.set(false);
        // Mirror the shape the API returns for an image value.
        this.values[field.name] = { id: info.id, url: info.upload_url.split('?')[0] };
        input.value = '';
      },
      error: (e) => {
        this.uploading.set(false);
        this.error.set(`Upload failed: ${message(e)}`);
      },
    });
  }

  save() {
    const normalized = this.normalize();
    if (normalized === null) {
      return;
    }

    this.error.set('');

    // Subscribe per branch: the create and update calls return different observable
    // types, which cannot be unioned into a single `subscribe` call.
    const id = this.itemId;
    if (id === null) {
      this.collectionsService.createCollectionItem(this.collectionName, normalized).subscribe({
        next: () => this.goBackToList(),
        error: (e) => this.error.set(`Save failed: ${message(e)}`),
      });
    } else {
      this.collectionsService.updateCollectionItem(this.collectionName, id, normalized).subscribe({
        next: () => this.goBackToList(),
        error: (e) => this.error.set(`Save failed: ${message(e)}`),
      });
    }
  }

  private goBackToList() {
    this.router.navigate(['/collections', this.collectionName]);
  }

  /**
   * Fold the JSON textareas back into the value map. Returns `null` (after setting the
   * error) when a buffer is not valid JSON, so a typo cannot be saved as something else.
   */
  private normalize(): CollectionValue | null {
    this.error.set('');
    const result: CollectionValue = { ...this.values };
    for (const field of this.schema) {
      if (!isArrayFieldSchema(field.field_type)) {
        continue;
      }
      const text = (this.arrayText[field.name] ?? '').trim();
      if (text === '') {
        result[field.name] = [];
        continue;
      }
      try {
        const parsed = JSON.parse(text);
        if (!Array.isArray(parsed)) {
          this.error.set(`Field '${field.name}': expected a JSON array`);
          return null;
        }
        result[field.name] = parsed as FieldValue;
      } catch {
        this.error.set(`Field '${field.name}': invalid JSON`);
        return null;
      }
    }
    return result;
  }
}
