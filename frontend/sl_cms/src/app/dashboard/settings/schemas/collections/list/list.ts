import { Component, inject, signal } from '@angular/core';
import { BehaviorSubject, Observable, switchMap } from 'rxjs';
import { MatIconModule } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatTableModule } from '@angular/material/table';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';

import { Message, MessagePipe, failure } from 'app/core/i18n/message';
import { CollectionsService } from 'app/services/schema/collections.service';

@Component({
  // Distinct from the `app-list` used by dashboard/collections/list; two components
  // sharing a selector is ambiguous.
  selector: 'app-schema-collection-list',
  imports: [ MatTooltipModule,
    MatTableModule,
    MatIconModule,
    MatButtonModule,
    MessagePipe,
    RouterLink,
    TranslocoPipe,
  ],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class List {
  private collectionsService = inject(CollectionsService);
  private i18n = inject(TranslocoService);
  /** Re-emits to re-issue the list request after a successful delete. */
  private refresh = new BehaviorSubject<void>(undefined);
  public collectionNames: Observable<string[]> = this.refresh.pipe(
    switchMap(() => this.collectionsService.getAllCollectionNames()),
  );
  public displayedColumns: string[] = ['name', 'edit', 'delete'];
  public error = signal<Message | null>(null);

  delete(name: string) {
    if (!confirm(this.i18n.translate('content.deleteCollectionConfirm', { name }))) {
      return;
    }
    this.collectionsService.deleteCollection(name).subscribe({
      next: () => {
        this.error.set(null);
        this.refresh.next();
      },
      error: (e) => this.error.set(failure('content.deleteFailed', e)),
    });
  }
}
