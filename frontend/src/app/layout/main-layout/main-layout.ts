import { Component, OnInit, inject, signal } from '@angular/core';
import { Header } from '../header/header';
import { Sidebar } from '../sidebar/sidebar';
import { RouterModule } from '@angular/router';

import { AuthService } from 'app/core/auth/auth.service';

@Component({
  selector: 'app-main-layout',
  templateUrl: './main-layout.html',
  styleUrl: './main-layout.scss',
  imports: [Header, Sidebar, RouterModule],
})
export class MainLayout implements OnInit {
  /**
   * Whether the navigation is out of the way.
   *
   * On a narrow screen the sidebar takes the width a form needs, and an editor reading one screen
   * should be able to put it away; the header's menu button is the one control for it.
   */
  public sidebarHidden = signal(false);

  public toggleSidebar() {
    this.sidebarHidden.update((hidden) => !hidden);
  }

  private auth = inject(AuthService);

  /**
   * Complete a session that arrived as a token and nothing else.
   *
   * A page reload reads the account from storage, but the person who just followed a password
   * reset link has never signed in on this browser: the token is there and the record is not.
   * Every permission question is answered from the record, so without this the CMS would show
   * them the screens of a viewer until they signed in again.
   */
  ngOnInit() {
    this.auth.loadUserIfMissing().subscribe({
      // A token the server no longer accepts is the interceptor's business; this only ever adds
      // what the screens need, so a failure leaves the shell as it was.
      error: () => {},
    });
  }
}
