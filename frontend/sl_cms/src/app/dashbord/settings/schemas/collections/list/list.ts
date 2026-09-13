import { Component, inject } from '@angular/core';
import { Observable } from 'rxjs';
import { AsyncPipe } from '@angular/common';
import { MatIconModule } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatTableModule } from '@angular/material/table';
import { CollectionsService } from 'app/services/schema/collections.service';

@Component({
  selector: 'app-list',
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
  public collectionNames: Observable<string[]> = this.collectionsService.getAllCollectionNames();
  public displayedColumns: string[] = ['name', 'edit', 'delete'];
  ngOnInit() {
  }
}
