import { Component } from '@angular/core';
import { TranslocoPipe } from '@jsverse/transloco';

/**
 * The landing screen.
 *
 * Nothing is chosen yet when someone signs in, and there is no "dashboard" worth inventing: the
 * navigation is where the work starts, so this says so rather than showing a demo widget.
 */
@Component({
  selector: 'app-index',
  imports: [TranslocoPipe],
  templateUrl: './index.html',
  styleUrl: './index.scss',
})
export class Index {}
