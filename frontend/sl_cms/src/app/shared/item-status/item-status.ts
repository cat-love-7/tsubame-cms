import { Component, input } from '@angular/core';
import { TranslocoPipe } from '@jsverse/transloco';

import { ItemStatus } from 'app/models/item-status';

/**
 * Draft/published badge, shared by the item list and both editors so the wording and
 * colours cannot drift apart between screens.
 */
@Component({
  selector: 'app-item-status',
  imports: [TranslocoPipe],
  templateUrl: './item-status.html',
  styleUrl: './item-status.scss',
})
export class ItemStatusBadge {
  public status = input<ItemStatus>('draft');
}
