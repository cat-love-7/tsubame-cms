import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';

import { fieldCellStyle } from 'app/core/field-layout';
import { errorMessage as message } from 'app/core/http-error';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { CollectionValue } from 'app/models/values/collection';
import { FieldValue, withDefaults } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { ValueField } from 'app/shared/value-field/value-field';

/**
 * Create/edit one collection item.
 *
 * The form is driven by the collection schema: the shared `ValueField` renders the widget
 * each field type needs and owns its own input state. Values are sent **without type
 * tags**, and fields this editor cannot edit yet (composite fields) keep whatever the
 * server sent so saving never silently discards them.
 */
@Component({
  selector: 'app-item-edit',
  imports: [RouterLink, MatButtonModule, ValueField],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class Edit {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private collectionsService = inject(CollectionsService);

  public collectionName: string = this.route.snapshot.params['name'];
  private readonly itemId: number | null =
    this.route.snapshot.params['id'] === undefined
      ? null
      : Number(this.route.snapshot.params['id']);
  public readonly isNew = this.itemId === null;

  public schema: CollectionSchema = [];
  public values: CollectionValue = {};
  public error = signal('');
  /** Places each field on the shared 12-column grid, mirroring the schema editor. */
  public cellStyle = fieldCellStyle;

  /**
   * Per-field input problems reported by the value fields (invalid JSON, failed upload).
   * Saving is refused while any remain, so bad input is neither sent nor replaced by a
   * stale value without the user noticing.
   */
  private fieldErrors: { [field: string]: string } = {};

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
      next: (values) => (this.values = withDefaults(schema, values)),
      error: (e) => this.error.set(`Failed to load the item: ${message(e)}`),
    });
  }

  setValue(field: FieldSchema, value: FieldValue) {
    this.values[field.name] = value;
  }

  setFieldError(field: FieldSchema, message: string | null) {
    if (message) {
      this.fieldErrors[field.name] = message;
    } else {
      delete this.fieldErrors[field.name];
    }
  }

  save() {
    const problems = Object.values(this.fieldErrors);
    if (problems.length > 0) {
      this.error.set(problems[0]);
      return;
    }

    this.error.set('');
    const values: CollectionValue = { ...this.values };

    // Subscribe per branch: the create and update calls return different observable
    // types, which cannot be unioned into a single `subscribe` call.
    const id = this.itemId;
    if (id === null) {
      this.collectionsService.createCollectionItem(this.collectionName, values).subscribe({
        next: () => this.goBackToList(),
        error: (e) => this.error.set(`Save failed: ${message(e)}`),
      });
    } else {
      this.collectionsService.updateCollectionItem(this.collectionName, id, values).subscribe({
        next: () => this.goBackToList(),
        error: (e) => this.error.set(`Save failed: ${message(e)}`),
      });
    }
  }

  private goBackToList() {
    this.router.navigate(['/collections', this.collectionName]);
  }
}
