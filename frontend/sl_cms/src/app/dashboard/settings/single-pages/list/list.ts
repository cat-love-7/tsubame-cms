import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { BehaviorSubject, Observable, switchMap } from 'rxjs';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatTableModule } from '@angular/material/table';
import { MatTooltipModule } from '@angular/material/tooltip';

import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';

import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { SinglePagesService } from 'app/services/schema/single_pages.service';

@Component({
  selector: 'app-single-page-list',
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
export class List {
  private pages = inject(SinglePagesService);
  private router = inject(Router);
  private i18n = inject(TranslocoService);

  /** Re-issues the list request after a delete. */
  private refresh = new BehaviorSubject<void>(undefined);
  public pageNames: Observable<string[]> = this.refresh.pipe(
    switchMap(() => this.pages.listPageNames()),
  );
  public displayedColumns: string[] = ['name', 'content', 'schema', 'delete'];

  public newName = '';
  public error = signal<Message | null>(null);

  create() {
    const name = this.newName.trim();
    if (!name) {
      this.error.set(t('content.requiredPageName'));
      return;
    }
    this.error.set(null);
    // Start from an empty schema; fields are added on the schema screen this navigates to.
    this.pages.createPage(name, []).subscribe({
      next: () => {
        this.newName = '';
        this.router.navigate(['/settings/single-pages', name, 'schema']);
      },
      error: (e) => this.error.set(failure('content.createFailed', e)),
    });
  }

  delete(name: string) {
    if (!confirm(this.i18n.translate('content.deletePageConfirm', { name }))) {
      return;
    }
    this.pages.deletePage(name).subscribe({
      next: () => {
        this.error.set(null);
        this.refresh.next();
      },
      error: (e) => this.error.set(failure('content.deleteFailed', e)),
    });
  }
}
