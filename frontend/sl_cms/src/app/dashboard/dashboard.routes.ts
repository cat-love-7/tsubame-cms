import { Routes } from '@angular/router';

import { unsavedChangesGuard } from 'app/core/unsaved-changes.guard';

/**
 * The dashboard's routes, one lazy chunk per screen.
 *
 * `loadComponent` rather than a static import of fifteen components: as one chunk, a reader who
 * opens the image library downloads the schema editors, the account list and every other screen
 * with it. The guard is imported directly - it is a few lines, and it has to be there before the
 * first screen is activated.
 */
export const dashboardRoutes: Routes = [
  {
    path: '',
    loadComponent: () => import('./index').then((m) => m.Index),
  },
  // Content: list an arbitrary collection's items and edit one of them. The item editor
  // doubles as the create screen (no `id` in the path means "new").
  {
    path: 'collections/:name',
    loadComponent: () => import('./collections/list/list').then((m) => m.CollectionItemList),
  },
  // Both item screens hold edits until their save button is pressed, so leaving is asked about
  // (`unsavedChangesGuard`).
  {
    path: 'collections/:name/create',
    loadComponent: () => import('./collections/edit/edit').then((m) => m.CollectionItemEdit),
    canDeactivate: [unsavedChangesGuard],
  },
  {
    path: 'collections/:name/edit/:id',
    loadComponent: () => import('./collections/edit/edit').then((m) => m.CollectionItemEdit),
    canDeactivate: [unsavedChangesGuard],
  },
  // Single pages ("single documents"): one schema and exactly one item each, so there is no
  // create screen - but the overview says what is live and what has unpublished changes, and
  // releases content without opening the page.
  {
    path: 'single-pages',
    loadComponent: () => import('./single-pages/list/list').then((m) => m.SinglePageList),
  },
  {
    path: 'single-pages/:name',
    loadComponent: () => import('./single-pages/edit/edit').then((m) => m.SinglePageEdit),
    canDeactivate: [unsavedChangesGuard],
  },
  // The image library. Content rather than configuration: an editor uploads the images their
  // content uses, so it lives with the documents. The old path still answers, because a
  // bookmark is a link someone kept.
  {
    path: 'images',
    loadComponent: () => import('./settings/images/list/list').then((m) => m.ImageLibrary),
  },
  {
    path: 'settings/images',
    redirectTo: 'images',
  },
  // Schema editing.
  {
    path: 'settings/collections',
    loadComponent: () =>
      import('./settings/collections/list/list').then((m) => m.CollectionSchemaList),
  },
  {
    path: 'settings/collections/create',
    loadComponent: () =>
      import('./settings/collections/create/create').then((m) => m.CollectionSchemaCreate),
    canDeactivate: [unsavedChangesGuard],
  },
  {
    path: 'settings/collections/:name/schema',
    loadComponent: () =>
      import('./settings/collections/schema/schema').then((m) => m.CollectionSchemaEdit),
    canDeactivate: [unsavedChangesGuard],
  },
  // The addresses these screens used to have, under `settings/schemas/`: a bookmark is a link
  // someone kept, and so is an entry in the browser's history.
  {
    path: 'settings/schemas/collections',
    redirectTo: 'settings/collections',
  },
  {
    path: 'settings/schemas/collections/create',
    redirectTo: 'settings/collections/create',
  },
  {
    path: 'settings/schemas/collections/edit/:name',
    redirectTo: 'settings/collections/:name/schema',
  },
  {
    path: 'settings/single-pages',
    loadComponent: () =>
      import('./settings/single-pages/list/list').then((m) => m.SinglePageSchemaList),
  },
  {
    path: 'settings/single-pages/:name/schema',
    loadComponent: () =>
      import('./settings/single-pages/schema/schema').then((m) => m.SinglePageSchema),
    canDeactivate: [unsavedChangesGuard],
  },
  // Composite fields: reusable groups of fields that other schemas reference by id.
  {
    path: 'settings/composite-fields',
    loadComponent: () =>
      import('./settings/composite-fields/list/list').then((m) => m.CompositeFieldList),
  },
  {
    path: 'settings/composite-fields/:id/schema',
    loadComponent: () =>
      import('./settings/composite-fields/schema/schema').then((m) => m.CompositeFieldSchema),
    canDeactivate: [unsavedChangesGuard],
  },
  // Accounts, for administrators. The server enforces the same rule.
  {
    path: 'settings/users',
    loadComponent: () => import('./settings/users/list/list').then((m) => m.UsersList),
  },
  // Open to every signed-in account, read-only ones included.
  {
    path: 'account',
    loadComponent: () => import('./account/password/password').then((m) => m.Password),
  },
];
