import { Component, inject } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIcon, MatIconModule } from '@angular/material/icon';
import { MatTreeModule } from '@angular/material/tree';
import { RouterLink } from '@angular/router';
import { CollectionRepository } from 'app/repositories/schema/collections.repository';
import { CollectionsService } from 'app/services/schema/collections.service';
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
  providers: [
    {
        provide: CollectionRepository,
        useClass: CollectionRepository,
    },
    {
        provide: CollectionsService,
        useFactory: (collectionRepository: CollectionRepository) => {
            return new CollectionsService(collectionRepository);
        },
        deps: [CollectionRepository],
    }
  ]
})
export class Sidebar {
  private collectionsService = inject(CollectionsService);
  dataSource!: Observable<SidebarItem[]>;
  childrenAccessor = (node: SidebarItem) => node.children || [];
  hasChild = (_: number, node: SidebarItem) => !!node.children && node.children.length > 0;
  ngOnInit() {
    let list = combineLatest([
      this.collectionsService.getAllCollectionNames()
    ]).pipe(map(this.toTreeNodes));
    this.dataSource = list;
  }
  toTreeNodes(obs: any): SidebarItem[] {
    let collectionNames: string[] = obs[0];
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
              },
              {
                name: 'Composite Fields',
              }
            ],
          },
        ],
      },
    ]
  }
}

const testData: SidebarItem[] = [
  {
    name: 'Documents',
    children: [
      {
        name: 'Collections',
      },
      {
        name: 'Single Documents',
      },
    ],
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
          },
        ],
      },
    ],
  },
];
