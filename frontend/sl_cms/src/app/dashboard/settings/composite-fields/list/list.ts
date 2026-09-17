import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { BehaviorSubject, Observable, map, switchMap } from 'rxjs';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatTableModule } from '@angular/material/table';
import { MatTooltipModule } from '@angular/material/tooltip';

import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';

import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { CompositeFieldsService } from 'app/services/schema/composite_fields.service';

interface DefinitionRow {
  id: string;
  fields: string;
}

@Component({
  selector: 'app-composite-field-list',
  imports: [ MatTooltipModule,
    FormsModule,
    MatButtonModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatTableModule,
    MessagePipe,
    RouterLink,
    TranslocoPipe,
  ],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class CompositeFieldList {
  private compositeFields = inject(CompositeFieldsService);
  private router = inject(Router);
  private i18n = inject(TranslocoService);

  /** Re-issues the list request after a change. */
  private refresh = new BehaviorSubject<void>(undefined);
  public definitions: Observable<DefinitionRow[]> = this.refresh.pipe(
    switchMap(() => this.compositeFields.getAllCompositeFields()),
    map((all) =>
      Object.entries(all).map(([id, definition]) => ({
        id,
        fields: definition.map((field) => field.name).join(', ') || '—',
      })),
    ),
  );
  public displayedColumns: string[] = ['id', 'fields', 'schema', 'delete'];

  public newId = '';
  public error = signal<Message | null>(null);

  create() {
    const id = this.newId.trim();
    if (!id) {
      this.error.set(t('content.requiredCompositeId'));
      return;
    }
    this.error.set(null);
    // Start from an empty definition; fields are added on the screen this navigates to.
    this.compositeFields.createCompositeField(id, []).subscribe({
      next: () => {
        this.compositeFields.invalidate();
        this.newId = '';
        this.router.navigate(['/settings/composite-fields', id, 'schema']);
      },
      error: (e) => this.error.set(failure('content.createFailed', e)),
    });
  }

  delete(id: string) {
    if (!confirm(this.i18n.translate('content.deleteCompositeConfirm', { id }))) {
      return;
    }
    this.compositeFields.deleteCompositeField(id).subscribe({
      next: () => {
        this.error.set(null);
        // Drop the cached definitions before re-reading, or the list would serve the
        // deleted entry from the cache.
        this.compositeFields.invalidate();
        this.refresh.next();
      },
      error: (e) => this.error.set(failure('content.deleteFailed', e)),
    });
  }
}
