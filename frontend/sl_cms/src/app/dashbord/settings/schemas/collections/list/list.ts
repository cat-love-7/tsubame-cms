import { Component, inject, signal } from '@angular/core';
import { BehaviorSubject, Observable, switchMap } from 'rxjs';
import { MatIconModule } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatTableModule } from '@angular/material/table';
import { CollectionsService } from 'app/services/schema/collections.service';

@Component({
  // Distinct from the `app-list` used by dashbord/collections/list; two components
  // sharing a selector is ambiguous.
  selector: 'app-schema-collection-list',
  imports: [
    MatTableModule,
    MatIconModule,
    MatButtonModule,
    RouterLink,
  ],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class List {
  private collectionsService = inject(CollectionsService);
  /** Re-emits to re-issue the list request after a successful delete. */
  private refresh = new BehaviorSubject<void>(undefined);
  public collectionNames: Observable<string[]> = this.refresh.pipe(
    switchMap(() => this.collectionsService.getAllCollectionNames()),
  );
  public displayedColumns: string[] = ['name', 'edit', 'delete'];
  public error = signal('');

  delete(name: string) {
    if (!confirm(`Delete collection "${name}" and all of its items?`)) {
      return;
    }
    this.collectionsService.deleteCollection(name).subscribe({
      next: () => {
        this.error.set('');
        this.refresh.next();
      },
      error: (e) => this.error.set(`Delete failed: ${e?.error ?? e?.message ?? e}`),
    });
  }
}
