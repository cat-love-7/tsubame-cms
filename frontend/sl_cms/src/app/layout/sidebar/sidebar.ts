import { Component, inject } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIcon, MatIconModule } from '@angular/material/icon';
import { MatTreeModule } from '@angular/material/tree';
import { RouterLink } from '@angular/router';
import { TranslocoPipe } from '@jsverse/transloco';
import { AuthService } from 'app/core/auth/auth.service';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single_pages.service';
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
  imports: [
    MatTreeModule,
    MatButtonModule,
    MatIconModule,
    RouterLink,
    TranslocoPipe,
  ],
})
export class Sidebar {
  private collectionsService = inject(CollectionsService);
  private singlePagesService = inject(SinglePagesService);
  private auth = inject(AuthService);
  dataSource!: Observable<SidebarItem[]>;
  childrenAccessor = (node: SidebarItem) => node.children || [];
  hasChild = (_: number, node: SidebarItem) => !!node.children && node.children.length > 0;
  ngOnInit() {
    let list = combineLatest([
      this.collectionsService.getAllCollectionNames(),
      this.singlePagesService.listPageNames(),
    ]).pipe(map((names) => this.toTreeNodes(names)));
    this.dataSource = list;
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
            link: singlePageLinks.length === 0 ? '/settings/single-pages' : undefined,
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
              {
                name: 'Images',
                nameKey: 'content.images',
                link: '/settings/images',
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
