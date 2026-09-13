import { RouterModule, Routes } from "@angular/router";
import { Index } from "./index";
import { NgModule } from "@angular/core";
import { List as schemaCollectionList } from "./settings/schemas/collections/list/list";
import { CollectionRepository } from "app/repositories/schema/collections.repository";
import { CollectionsService } from "app/services/schema/collections.service";
import { Edit } from "./settings/schemas/collections/edit/edit";
import { CompositeFieldRepository } from "app/repositories/schema/composite_fields.repository";
import { CompositeFieldsService } from "app/services/schema/composite_fields.service";
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
        path: 'settings/schemas/collections/edit/:name',
        component: Edit
    }
];

@NgModule({
    declarations: [],
    imports: [RouterModule.forChild(dashbordRoutes)],
    exports: [RouterModule],
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
        },
        {
            provide: CompositeFieldRepository,
            useClass: CompositeFieldRepository,
        },
        {
            provide: CompositeFieldsService,
            useFactory: (compositeFieldRepository: CompositeFieldRepository) => {
                return new CompositeFieldsService(compositeFieldRepository);
            },
            deps: [CompositeFieldRepository],
        }
    ]
})

export class DashbordRoutingModule { }