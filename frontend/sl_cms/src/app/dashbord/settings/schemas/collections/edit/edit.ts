import { Component, inject, input } from '@angular/core';
import { EditSchema } from "../../shared/edit-schema/edit-schema";
import { CollectionsService } from 'app/services/schema/collections.service';
import { Observable } from 'rxjs';
import { CollectionSchema } from 'app/models/schema/collection';
import { ActivatedRoute } from '@angular/router';
import { AsyncPipe, NgIf } from '@angular/common';

@Component({
  selector: 'app-edit',
  imports: [
    EditSchema,
    AsyncPipe,
],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class Edit {
  private route = inject(ActivatedRoute);
  private collectionsService = inject(CollectionsService);
  public collectionSchema: Observable<CollectionSchema> = this.collectionsService.getCollectionSchema(this.route.snapshot.params['name']);

}
