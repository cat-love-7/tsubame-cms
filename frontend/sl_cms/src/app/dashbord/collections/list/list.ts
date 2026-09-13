import { Component, inject } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { CollectionSchema } from 'app/models/schema/collection';
import { CollectionsService } from 'app/services/schema/collections.service';
import { Observable } from 'rxjs';

@Component({
  selector: 'app-list',
  imports: [],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class List {
  private route = inject(ActivatedRoute);
  private collectionsService = inject(CollectionsService);
  public collectionSchema: Observable<CollectionSchema> = this.collectionsService.getCollectionSchema(this.route.snapshot.params['name']);
}
