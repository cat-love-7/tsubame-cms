import { RouterModule, Routes } from "@angular/router";
import { Index } from "./index";
import { NgModule } from "@angular/core";
import { List as schemaCollectionList } from "./settings/schemas/collections/list/list";
import { Edit as SchemaEdit } from "./settings/schemas/collections/edit/edit";
import { Create as SchemaCreate } from "./settings/schemas/collections/create/create";
import { List as CollectionList } from "./collections/list/list";
import { Edit as ItemEdit } from "./collections/edit/edit";
import { List as SinglePageSchemaList } from "./settings/single-pages/list/list";
import { List as SinglePageStatusList } from "./single-pages/list/list";
import { Schema as SinglePageSchema } from "./settings/single-pages/schema/schema";
import { Edit as SinglePageEdit } from "./single-pages/edit/edit";
import { List as CompositeFieldList } from "./settings/composite-fields/list/list";
import { Schema as CompositeFieldSchema } from "./settings/composite-fields/schema/schema";
import { List as ImageLibrary } from "./settings/images/list/list";
import { List as UsersList } from "./settings/users/list/list";
import { Password } from "./account/password/password";


const dashboardRoutes: Routes = [
    {
        path: '',
        component: Index
    },
    // Content: list an arbitrary collection's items and edit one of them. `Edit`
    // doubles as the create screen (no `id` in the path means "new").
    {
        path: 'collections/:name',
        component: CollectionList
    },
    {
        path: 'collections/:name/create',
        component: ItemEdit
    },
    {
        path: 'collections/:name/edit/:id',
        component: ItemEdit
    },
    // Single pages ("single documents"): one schema and exactly one item each, so there is no
    // create screen - but the overview says what is live and what has unpublished changes, and
    // releases content without opening the page.
    {
        path: 'single-pages',
        component: SinglePageStatusList
    },
    {
        path: 'single-pages/:name',
        component: SinglePageEdit
    },
    // The image library. Content rather than configuration: an editor uploads the images their
    // content uses, so it lives with the documents. The old path still answers, because a
    // bookmark is a link someone kept.
    {
        path: 'images',
        component: ImageLibrary
    },
    {
        path: 'settings/images',
        redirectTo: 'images'
    },
    // Schema editing.
    {
        path: 'settings/schemas/collections',
        component: schemaCollectionList
    },
    {
        path: 'settings/schemas/collections/create',
        component: SchemaCreate
    },
    {
        path: 'settings/schemas/collections/edit/:name',
        component: SchemaEdit
    },
    {
        path: 'settings/single-pages',
        component: SinglePageSchemaList
    },
    {
        path: 'settings/single-pages/:name/schema',
        component: SinglePageSchema
    },
    // Composite fields: reusable groups of fields that other schemas reference by id.
    {
        path: 'settings/composite-fields',
        component: CompositeFieldList
    },
    {
        path: 'settings/composite-fields/:id/schema',
        component: CompositeFieldSchema
    },
    // Accounts, for administrators. The server enforces the same rule.
    {
        path: 'settings/users',
        component: UsersList
    },
    // Open to every signed-in account, read-only ones included.
    {
        path: 'account',
        component: Password
    }
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

export class DashboardRoutingModule { }
