import { Component, ChangeDetectionStrategy } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';

@Component({
  selector: 'app-index',
  imports: [
    MatButtonModule
  ],
  templateUrl: './index.html',
  changeDetection: ChangeDetectionStrategy.Eager,
  styleUrl: './index.scss',
})
export class Index {

}
