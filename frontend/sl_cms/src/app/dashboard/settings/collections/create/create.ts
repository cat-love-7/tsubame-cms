import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { TranslocoPipe } from '@jsverse/transloco';

import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { HasUnsavedChanges } from 'app/core/unsaved-changes.guard';
import { CollectionsService } from 'app/services/schema/collections.service';

@Component({
  selector: 'app-collection-schema-create',
  imports: [
    FormsModule,
    MatButtonModule,
    MatFormFieldModule,
    MatInputModule,
    MessagePipe,
    RouterLink,
    TranslocoPipe,
  ],
  templateUrl: './create.html',
  styleUrl: './create.scss',
})
export class CollectionSchemaCreate implements HasUnsavedChanges {
  private collectionsService = inject(CollectionsService);
  private router = inject(Router);
  public name = '';
  public error = signal<Message | null>(null);

  /** A name typed but not created yet is work that would be lost (see `unsavedChangesGuard`). */
  hasUnsavedChanges(): boolean {
    return this.name.trim().length > 0;
  }

  create() {
    const name = this.name.trim();
    if (!name) {
      this.error.set(t('content.requiredCollectionName'));
      return;
    }
    this.error.set(null);
    // Start from an empty schema; fields are added on the edit page.
    this.collectionsService.createCollection(name, []).subscribe({
      next: () => {
        // `navigate` (not `navigateByUrl`) so the name is URL-encoded.
        void this.router.navigate(['/settings/collections', name, 'schema']);
      },
      error: (e) => this.error.set(failure('content.createFailed', e)),
    });
  }
}
