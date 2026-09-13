import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { BehaviorSubject, switchMap } from 'rxjs';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';

import { errorMessage as message } from 'app/core/http-error';
import { CollectionSchema } from 'app/models/schema/collection';
import { CollectionItemEntry } from 'app/models/values/collection';
import { formatFieldValue } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';

@Component({
  selector: 'app-collection-items',
  imports: [MatButtonModule, MatIconModule, RouterLink],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class List {
  private route = inject(ActivatedRoute);
  private collectionsService = inject(CollectionsService);

  public collectionName: string = this.route.snapshot.params['name'];
  public schema: CollectionSchema = [];
  public items: CollectionItemEntry[] = [];
  public error = signal('');
  /** Exposed for the template. */
  public format = formatFieldValue;

  /** Re-issues the list request after a delete. */
  private refresh = new BehaviorSubject<void>(undefined);

  constructor() {
    this.collectionsService.getCollectionSchema(this.collectionName).subscribe({
      next: (schema) => (this.schema = schema),
      error: (e) => this.error.set(`Failed to load the schema: ${message(e)}`),
    });

    this.refresh
      .pipe(switchMap(() => this.collectionsService.listCollectionItems(this.collectionName)))
      .subscribe({
        next: (items) => (this.items = items),
        error: (e) => this.error.set(`Failed to load the items: ${message(e)}`),
      });
  }

  delete(id: number) {
    if (!confirm(`Delete item ${id}?`)) {
      return;
    }
    this.collectionsService.deleteCollectionItem(this.collectionName, id).subscribe({
      next: () => {
        this.error.set('');
        this.refresh.next();
      },
      error: (e) => this.error.set(`Delete failed: ${message(e)}`),
    });
  }
}
