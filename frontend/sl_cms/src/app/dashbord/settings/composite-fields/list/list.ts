import { Component, inject, signal, ChangeDetectionStrategy } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { BehaviorSubject, Observable, map, switchMap } from 'rxjs';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatTableModule } from '@angular/material/table';

import { errorMessage as message } from 'app/core/http-error';
import { CompositeFieldsService } from 'app/services/schema/composite_fields.service';

interface DefinitionRow {
  id: string;
  fields: string;
}

@Component({
  selector: 'app-composite-field-list',
  imports: [
    FormsModule,
    RouterLink,
    MatButtonModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatTableModule,
  ],
  templateUrl: './list.html',
  changeDetection: ChangeDetectionStrategy.Eager,
  styleUrl: './list.scss',
})
export class List {
  private compositeFields = inject(CompositeFieldsService);
  private router = inject(Router);

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
  public error = signal('');

  create() {
    const id = this.newId.trim();
    if (!id) {
      this.error.set('Composite field id is required');
      return;
    }
    this.error.set('');
    // Start from an empty definition; fields are added on the screen this navigates to.
    this.compositeFields.createCompositeField(id, []).subscribe({
      next: () => {
        this.compositeFields.invalidate();
        this.newId = '';
        this.router.navigate(['/settings/composite-fields', id, 'schema']);
      },
      error: (e) => this.error.set(`Create failed: ${message(e)}`),
    });
  }

  delete(id: string) {
    if (!confirm(`Delete the composite field "${id}"?`)) {
      return;
    }
    this.compositeFields.deleteCompositeField(id).subscribe({
      next: () => {
        this.error.set('');
        // Drop the cached definitions before re-reading, or the list would serve the
        // deleted entry from the cache.
        this.compositeFields.invalidate();
        this.refresh.next();
      },
      error: (e) => this.error.set(`Delete failed: ${message(e)}`),
    });
  }
}
