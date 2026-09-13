import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';

import { errorMessage as message } from 'app/core/http-error';
import { FieldSchema } from 'app/models/schema/fields';
import { SinglePagesService } from 'app/services/schema/single_pages.service';
import { EditSchema } from '../../schemas/shared/edit-schema/edit-schema';

/** Schema editor for one single page. Reuses the schema editor the collections use. */
@Component({
  selector: 'app-single-page-schema',
  imports: [RouterLink, MatButtonModule, EditSchema],
  templateUrl: './schema.html',
  styleUrl: './schema.scss',
})
export class Schema {
  private route = inject(ActivatedRoute);
  private pages = inject(SinglePagesService);

  public pageName: string = this.route.snapshot.params['name'];
  public schema: FieldSchema[] = [];
  public status = signal('');
  public error = signal('');

  constructor() {
    this.pages.getPageSchema(this.pageName).subscribe({
      next: (schema) => (this.schema = schema),
      error: (e) => this.error.set(`Failed to load the schema: ${message(e)}`),
    });
  }

  save(schema: FieldSchema[]) {
    this.pages.updatePageSchema(this.pageName, schema).subscribe({
      next: () => {
        this.error.set('');
        this.status.set('Saved');
      },
      error: (e) => {
        this.status.set('');
        this.error.set(`Save failed: ${message(e)}`);
      },
    });
  }
}
