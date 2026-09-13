import { Component, ChangeDetectionStrategy } from '@angular/core';
import { Header } from '../header/header';
import { Sidebar } from '../sidebar/sidebar';
import { RouterModule } from '@angular/router';

@Component({
  selector: 'app-main-layout',
  templateUrl: './main-layout.html',
  styleUrl: './main-layout.scss',
  changeDetection: ChangeDetectionStrategy.Eager,
  imports: [
    Header,
    Sidebar,
    RouterModule,
  ]
})
export class MainLayout {

}
