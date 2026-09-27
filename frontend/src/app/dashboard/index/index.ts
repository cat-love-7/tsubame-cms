import { Component, inject } from '@angular/core';
import { TranslocoPipe } from '@jsverse/transloco';

import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';

/**
 * The landing screen.
 *
 * Nothing is chosen yet when someone signs in, and there is no "dashboard" worth inventing: the
 * navigation is where the work starts, so this says so rather than showing a demo widget. What it
 * does say is which deployment this is - the one question a reader with two of these open has, and
 * the one the app bar answers the same way.
 */
@Component({
  selector: 'app-index',
  imports: [TranslocoPipe],
  templateUrl: './index.html',
  styleUrl: './index.scss',
})
export class Index {
  /**
   * What the deployment calls itself, or null where it never said: the heading is then the
   * product's name, which is what this screen showed before a deployment could have one.
   */
  public siteName = inject(CapabilitiesService).siteName;
}
