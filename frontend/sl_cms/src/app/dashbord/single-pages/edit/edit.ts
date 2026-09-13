import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';

import { fieldCellStyle } from 'app/core/field-layout';
import { errorMessage as message } from 'app/core/http-error';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { ContentValue } from 'app/models/values/collection';
import { FieldValue, withDefaults } from 'app/models/values/fields';
import { SinglePagesService } from 'app/services/schema/single_pages.service';
import { ValueField } from 'app/shared/value-field/value-field';

/**
 * Edit the content of one single page.
 *
 * A single page has exactly one item, so there is no list and no create/delete here —
 * only the form. It shares `ValueField` and the layout grid with the collection editor.
 */
@Component({
  selector: 'app-single-page-edit',
  imports: [RouterLink, MatButtonModule, ValueField],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class Edit {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private pages = inject(SinglePagesService);

  public pageName: string = this.route.snapshot.params['name'];
  public schema: CollectionSchema = [];
  public values: ContentValue = {};
  public error = signal('');
  public cellStyle = fieldCellStyle;

  /** Per-field problems reported by the value fields; saving is refused while any remain. */
  private fieldErrors: { [field: string]: string } = {};

  constructor() {
    this.pages.getPageSchema(this.pageName).subscribe({
      next: (schema) => {
        this.schema = schema;
        this.loadItem(schema);
      },
      error: (e) => this.error.set(`Failed to load the schema: ${message(e)}`),
    });
  }

  private loadItem(schema: CollectionSchema) {
    this.pages.getPageItem(this.pageName).subscribe({
      next: (values) => (this.values = withDefaults(schema, values)),
      error: (e) => this.error.set(`Failed to load the content: ${message(e)}`),
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
    this.pages.updatePageItem(this.pageName, { ...this.values }).subscribe({
      next: () => {
        this.error.set('');
        this.router.navigate(['/settings/single-pages']);
      },
      error: (e) => this.error.set(`Save failed: ${message(e)}`),
    });
  }
}
