import { RouterModule, Routes } from "@angular/router";
import { Index } from "./index";
import { NgModule } from "@angular/core";
import { List as schemaCollectionList } from "./settings/schemas/collections/list/list";
import { Edit as SchemaEdit } from "./settings/schemas/collections/edit/edit";
import { Create as SchemaCreate } from "./settings/schemas/collections/create/create";
import { List as CollectionList } from "./collections/list/list";
import { Edit as ItemEdit } from "./collections/edit/edit";


const dashbordRoutes: Routes = [
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
    }
];

/**
 * Only routing lives here now. `CollectionRepository`, `CollectionsService` and the
 * composite-field equivalents are `providedIn: 'root'`, so re-declaring them here (and
 * again on `Sidebar`) only created duplicate instances.
 */
@NgModule({
    declarations: [],
    imports: [RouterModule.forChild(dashbordRoutes)],
    exports: [RouterModule],
})

export class DashbordRoutingModule { }
