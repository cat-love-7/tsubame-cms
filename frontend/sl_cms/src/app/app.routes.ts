import { Routes } from '@angular/router';
import { MainLayout } from './layout/main-layout/main-layout';
import { authGuard } from './core/auth/auth.guard';

export const routes: Routes = [
    // Public: the sign-in page renders outside the CMS shell. Lazily loaded so its
    // Material form components stay out of the initial bundle.
    {
        path: 'login',
        loadComponent: () => import('./core/auth/login/login').then(m => m.Login)
    },
    // A reset link is opened by someone who cannot sign in, so this screen is public too.
    {
        path: 'reset-password',
        loadComponent: () =>
            import('./core/auth/password-reset/password-reset').then(m => m.PasswordReset)
    },
    // Where the identity provider sends the browser back to. Public for the same reason: the code
    // in the address is the credential, and there is no session yet.
    {
        path: 'auth/callback',
        loadComponent: () => import('./core/auth/callback/callback').then(m => m.AuthCallback)
    },
    // Everything else requires a session; the guard redirects to /login otherwise.
    {
        path: '',
        component: MainLayout,
        canActivate: [authGuard],
        children: [
            {
                path: '',
                loadChildren: () => import('./dashboard/dashboard.routes').then((m) => m.dashboardRoutes)
            }
        ]
    }
];
