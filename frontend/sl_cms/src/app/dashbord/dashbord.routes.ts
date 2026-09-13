import { RouterModule, Routes } from "@angular/router";
import { Index } from "./index";
import { NgModule } from "@angular/core";
import { List as schemaCollectionList } from "./settings/schemas/collections/list/list";
import { Edit } from "./settings/schemas/collections/edit/edit";
import { Create } from "./settings/schemas/collections/create/create";
import { List as CollectionList } from "./collections/list/list";


const dashbordRoutes: Routes = [
    {
        path: '',
        component: Index
    },
    {
        path: 'collections/:name',
        component: CollectionList
    },
    {
        path: 'settings/schemas/collections',
        component: schemaCollectionList
    },
    {
        path: 'settings/schemas/collections/create',
        component: Create
    },
    {
        path: 'settings/schemas/collections/edit/:name',
        component: Edit
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
