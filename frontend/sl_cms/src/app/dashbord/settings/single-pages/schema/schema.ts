import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { TranslocoPipe } from '@jsverse/transloco';

import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { FieldSchema } from 'app/models/schema/fields';
import { SinglePagesService } from 'app/services/schema/single_pages.service';
import { EditSchema } from '../../schemas/shared/edit-schema/edit-schema';

/** Schema editor for one single page. Reuses the schema editor the collections use. */
@Component({
  selector: 'app-single-page-schema',
  imports: [EditSchema, MatButtonModule, MessagePipe, RouterLink, TranslocoPipe],
  templateUrl: './schema.html',
  styleUrl: './schema.scss',
})
export class Schema {
  private route = inject(ActivatedRoute);
  private pages = inject(SinglePagesService);

  public pageName: string = this.route.snapshot.params['name'];
  public schema = signal<FieldSchema[]>([]);
  public status = signal<Message | null>(null);
  public error = signal<Message | null>(null);

  constructor() {
    this.pages.getPageSchema(this.pageName).subscribe({
      next: (schema) => this.schema.set(schema),
      error: (e) => this.error.set(failure('content.failedToLoadSchema', e)),
    });
  }

  save(schema: FieldSchema[]) {
    this.pages.updatePageSchema(this.pageName, schema).subscribe({
      next: () => {
        this.error.set(null);
        this.status.set(t('common.saved'));
      },
      error: (e) => {
        this.status.set(null);
        this.error.set(failure('content.saveFailed', e));
      },
    });
  }
}
