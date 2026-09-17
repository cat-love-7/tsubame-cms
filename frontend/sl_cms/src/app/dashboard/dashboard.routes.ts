import { NgModule } from '@angular/core';
import { RouterModule, Routes } from '@angular/router';

import { unsavedChangesGuard } from 'app/core/unsaved-changes.guard';

import { Password } from './account/password/password';
import { CollectionItemEdit } from './collections/edit/edit';
import { CollectionItemList } from './collections/list/list';
import { Index } from './index';
import { CollectionSchemaCreate } from './settings/schemas/collections/create/create';
import { CollectionSchemaEdit } from './settings/schemas/collections/edit/edit';
import { CollectionSchemaList } from './settings/schemas/collections/list/list';
import { CompositeFieldList } from './settings/composite-fields/list/list';
import { CompositeFieldSchema } from './settings/composite-fields/schema/schema';
import { ImageLibrary } from './settings/images/list/list';
import { SinglePageSchemaList } from './settings/single-pages/list/list';
import { SinglePageSchema } from './settings/single-pages/schema/schema';
import { UsersList } from './settings/users/list/list';
import { SinglePageEdit } from './single-pages/edit/edit';
import { SinglePageList } from './single-pages/list/list';

const dashboardRoutes: Routes = [
  {
    path: '',
    component: Index,
  },
  // Content: list an arbitrary collection's items and edit one of them. The item editor
  // doubles as the create screen (no `id` in the path means "new").
  {
    path: 'collections/:name',
    component: CollectionItemList,
  },
  // Both item screens hold edits until their save button is pressed, so leaving is asked about
  // (`unsavedChangesGuard`).
  {
    path: 'collections/:name/create',
    component: CollectionItemEdit,
    canDeactivate: [unsavedChangesGuard],
  },
  {
    path: 'collections/:name/edit/:id',
    component: CollectionItemEdit,
    canDeactivate: [unsavedChangesGuard],
  },
  // Single pages ("single documents"): one schema and exactly one item each, so there is no
  // create screen - but the overview says what is live and what has unpublished changes, and
  // releases content without opening the page.
  {
    path: 'single-pages',
    component: SinglePageList,
  },
  {
    path: 'single-pages/:name',
    component: SinglePageEdit,
    canDeactivate: [unsavedChangesGuard],
  },
  // The image library. Content rather than configuration: an editor uploads the images their
  // content uses, so it lives with the documents. The old path still answers, because a
  // bookmark is a link someone kept.
  {
    path: 'images',
    component: ImageLibrary,
  },
  {
    path: 'settings/images',
    redirectTo: 'images',
  },
  // Schema editing.
  {
    path: 'settings/schemas/collections',
    component: CollectionSchemaList,
  },
  {
    path: 'settings/schemas/collections/create',
    component: CollectionSchemaCreate,
  },
  {
    path: 'settings/schemas/collections/edit/:name',
    component: CollectionSchemaEdit,
  },
  {
    path: 'settings/single-pages',
    component: SinglePageSchemaList,
  },
  {
    path: 'settings/single-pages/:name/schema',
    component: SinglePageSchema,
  },
  // Composite fields: reusable groups of fields that other schemas reference by id.
  {
    path: 'settings/composite-fields',
    component: CompositeFieldList,
  },
  {
    path: 'settings/composite-fields/:id/schema',
    component: CompositeFieldSchema,
  },
  // Accounts, for administrators. The server enforces the same rule.
  {
    path: 'settings/users',
    component: UsersList,
  },
  // Open to every signed-in account, read-only ones included.
  {
    path: 'account',
    component: Password,
  },
];

/**
 * Only routing lives here now. `CollectionRepository`, `CollectionsService` and the
 * composite-field equivalents are `providedIn: 'root'`, so re-declaring them here (and
 * again on `Sidebar`) only created duplicate instances.
 */
@NgModule({
  declarations: [],
  imports: [RouterModule.forChild(dashboardRoutes)],
  exports: [RouterModule],
})
export class DashboardRoutingModule {}
