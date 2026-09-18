import { Component, inject, Injector } from '@angular/core';
import { toObservable } from '@angular/core/rxjs-interop';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatTreeModule } from '@angular/material/tree';
import { MatTooltipModule } from '@angular/material/tooltip';
import { RouterLink, RouterLinkActive } from '@angular/router';
import { TranslocoPipe } from '@jsverse/transloco';
import { AuthService } from 'app/core/auth/auth.service';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single-pages.service';
import { map, Observable, combineLatest} from 'rxjs';

interface SidebarItem {
  /** Data: a collection or page name comes from the API and is shown as it is. */
  name: string;
  /** Our own navigation label, which lives in the catalogs instead of here. */
  nameKey?: string;
  children?: SidebarItem[];
  link? : string;
}
@Component({
  selector: 'app-sidebar',
  templateUrl: './sidebar.html',
  styleUrl: './sidebar.scss',
  imports: [ MatTooltipModule,
    MatTreeModule,
    MatButtonModule,
    MatIconModule,
    RouterLink,
    RouterLinkActive,
    TranslocoPipe,
  ],
})
export class Sidebar {
  private collectionsService = inject(CollectionsService);
  private singlePagesService = inject(SinglePagesService);
  private auth = inject(AuthService);
  private injector = inject(Injector);
  dataSource!: Observable<SidebarItem[]>;
  childrenAccessor = (node: SidebarItem) => node.children || [];
  hasChild = (_: number, node: SidebarItem) => !!node.children && node.children.length > 0;
  ngOnInit() {
    // The account is part of the stream, not read once: only an administrator is offered the
    // schema and account screens, and the profile arrives a moment after the shell does - a
    // refresh on a settings screen would otherwise draw a navigation with those links missing.
    const user = toObservable(this.auth.user, { injector: this.injector });
    this.dataSource = combineLatest([
      this.collectionsService.getAllCollectionNames(),
      this.singlePagesService.listPageNames(),
      user,
    ]).pipe(map(([collections, pages]) => this.toTreeNodes([collections, pages])));
  }
  toTreeNodes(obs: [string[], string[]]): SidebarItem[] {
    const [collectionNames, singlePageNames] = obs;
    // Only administrators may change schemas or accounts, so the links are not offered to
    // anyone else (the server refuses them regardless).
    const isAdmin = this.auth.isAdmin();
    // An empty "Single Documents" has no children, so point it at the screen where one
    // can be created rather than rendering a dead link.
    const singlePageLinks: SidebarItem[] = singlePageNames.map((name) => ({
      name,
      link: `/single-pages/${name}`,
    }));
    return [
      {
        name: 'Documents',
        nameKey: 'nav.documents',
        children: [
          {
            name: 'Collections',
            nameKey: 'content.collections',
            children: collectionNames.map((name: string) => ({
              name: name,
              link: `/collections/${name}`,
            })),
          },
          {
            name: 'Single pages',
            nameKey: 'content.singlePages',
            children: singlePageLinks,
            link: '/single-pages',
          },
          {
            name: 'Images',
            nameKey: 'content.images',
            link: '/images',
          },
        ],
      },
      {
        name: 'Settings',
        nameKey: 'nav.settings',
        children: [
          {
            name: 'Schemas',
            nameKey: 'nav.schemas',
            children: [
              {
                name: 'Collections',
                nameKey: 'content.collections',
                link: '/settings/schemas/collections',
              },
              {
                name: 'Single pages',
                nameKey: 'content.singlePages',
                link: '/settings/single-pages',
              },
              {
                name: 'Composite fields',
                nameKey: 'content.compositeFields',
                link: '/settings/composite-fields',
              },
              ...(isAdmin
                ? [
                    {
                      name: 'Accounts',
                      nameKey: 'accounts.title',
                      link: '/settings/users',
                    },
                  ]
                : []),
            ],
          },
        ],
      },
    ]
  }
}
