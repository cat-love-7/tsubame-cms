import { Component, inject } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIcon, MatIconModule } from '@angular/material/icon';
import { MatTreeModule } from '@angular/material/tree';
import { RouterLink } from '@angular/router';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single_pages.service';
import { map, Observable, combineLatest} from 'rxjs';

interface SidebarItem {
  name: string;
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
  ],
})
export class Sidebar {
  private collectionsService = inject(CollectionsService);
  private singlePagesService = inject(SinglePagesService);
  dataSource!: Observable<SidebarItem[]>;
  childrenAccessor = (node: SidebarItem) => node.children || [];
  hasChild = (_: number, node: SidebarItem) => !!node.children && node.children.length > 0;
  ngOnInit() {
    let list = combineLatest([
      this.collectionsService.getAllCollectionNames(),
      this.singlePagesService.listPageNames(),
    ]).pipe(map(this.toTreeNodes));
    this.dataSource = list;
  }
  toTreeNodes(obs: [string[], string[]]): SidebarItem[] {
    const [collectionNames, singlePageNames] = obs;
    // An empty "Single Documents" has no children, so point it at the screen where one
    // can be created rather than rendering a dead link.
    const singlePageLinks: SidebarItem[] = singlePageNames.map((name) => ({
      name,
      link: `/single-pages/${name}`,
    }));
    return [
      {
        name: 'Documents',
        children: [
          {
          name: 'Collections',
          children: collectionNames.map((name: string) => ({
            name: name,
            link: `/collections/${name}`,
          })),
          },
          {
            name: 'Single Documents',
            children: singlePageLinks,
            link: singlePageLinks.length === 0 ? '/settings/single-pages' : undefined,
          },
        ]
      },
      {
        name: 'Settings',
        children: [
          {
            name: 'Schemas',
            children: [
              {
                name: 'Collections',
                link: '/settings/schemas/collections',
              },
              {
                name: 'Single Documents',
                link: '/settings/single-pages',
              },
              {
                name: 'Composite Fields',
                link: '/settings/composite-fields',
              }
            ],
          },
        ],
      },
    ]
  }
}
