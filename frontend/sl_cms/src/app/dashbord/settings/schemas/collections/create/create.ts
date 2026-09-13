import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { CollectionsService } from 'app/services/schema/collections.service';

@Component({
  selector: 'app-create-collection',
  imports: [
    FormsModule,
    RouterLink,
    MatButtonModule,
    MatFormFieldModule,
    MatInputModule,
  ],
  templateUrl: './create.html',
  styleUrl: './create.scss',
})
export class Create {
  private collectionsService = inject(CollectionsService);
  private router = inject(Router);
  public name = '';
  public error = signal('');

  create() {
    const name = this.name.trim();
    if (!name) {
      this.error.set('Collection name is required');
      return;
    }
    this.error.set('');
    // Start from an empty schema; fields are added on the edit page.
    this.collectionsService.createCollection(name, []).subscribe({
      next: () => {
        // `navigate` (not `navigateByUrl`) so the name is URL-encoded.
        this.router.navigate(['/settings/schemas/collections/edit', name]);
      },
      error: (e) => this.error.set(`Create failed: ${e?.error ?? e?.message ?? e}`),
    });
  }
}
