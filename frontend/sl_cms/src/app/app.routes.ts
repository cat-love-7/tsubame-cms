import { Routes } from '@angular/router';
import { MainLayout } from './layout/main-layout/main-layout';

export const routes: Routes = [
    {
        path: '',
        component: MainLayout,
        children: [
            {
                path: '',
                loadChildren: () => import('./dashbord/dashbord.routes').then(m => m.DashbordRoutingModule)
            }
        ]
    }
];
