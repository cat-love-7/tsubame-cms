import { HttpClient } from '@angular/common/http';
import { computed, inject, Injectable, signal } from '@angular/core';
import { Router } from '@angular/router';
import { Observable, tap } from 'rxjs';

export interface Permission {
  can_publish: boolean;
  can_edit: boolean;
  can_view: boolean;
}

export interface CurrentUser {
  id: string;
  email: string;
  is_admin: boolean;
  /** False for an account that has been disabled; it also cannot sign in. */
  is_active: boolean;
  permission: Permission;
  created_at: string;
  last_login: string | null;
}

/** The three capabilities, presented as roles on the account screens. */
export type Role = 'viewer' | 'editor' | 'publisher';

export function roleOf(user: CurrentUser): Role {
  if (!user.permission.can_edit) {
    return 'viewer';
  }
  return user.permission.can_publish ? 'publisher' : 'editor';
}

export function permissionFor(role: Role): Permission {
  return {
    can_view: true,
    // A viewer reads; an editor prepares; a publisher also releases.
    can_edit: role !== 'viewer',
    can_publish: role === 'publisher',
  };
}

interface LoginResponse {
  token: string;
  expires_at: string;
  user: CurrentUser;
}

const TOKEN_KEY = 'sl_cms.token';
const USER_KEY = 'sl_cms.user';

/**
 * Holds the bearer token and the signed-in user.
 *
 * The token is kept in `localStorage` so a reload does not sign the user out. That is a
 * deliberate trade-off: it is readable by scripts on this origin, so the token is scoped
 * to this application and should be short-lived (see `TOKEN_TTL_HOURS`).
 */
@Injectable({
  providedIn: 'root',
})
export class AuthService {
  private http = inject(HttpClient);
  private router = inject(Router);

  private readonly tokenSignal = signal<string | null>(readToken());
  private readonly userSignal = signal<CurrentUser | null>(readUser());

  readonly user = this.userSignal.asReadonly();
  readonly isAuthenticated = computed(() => this.tokenSignal() !== null);

  // What the signed-in account may do. The server is what enforces them; these only decide
  // which controls the screens offer, so a viewer is not invited to press a button that
  // would be refused.
  readonly canEdit = computed(() => {
    const user = this.userSignal();
    return user !== null && (user.is_admin || user.permission.can_edit);
  });
  readonly canPublish = computed(() => {
    const user = this.userSignal();
    return user !== null && (user.is_admin || user.permission.can_publish);
  });
  readonly isAdmin = computed(() => this.userSignal()?.is_admin ?? false);

  token(): string | null {
    return this.tokenSignal();
  }

  login(email: string, password: string): Observable<LoginResponse> {
    return this.http.post<LoginResponse>('/api/auth/login', { email, password }).pipe(
      tap((response) => {
        this.tokenSignal.set(response.token);
        this.userSignal.set(response.user);
        writeStorage(TOKEN_KEY, response.token);
        writeStorage(USER_KEY, JSON.stringify(response.user));
      }),
    );
  }

  logout(): void {
    this.clear();
    this.router.navigate(['/login']);
  }

  /** Drop the session without navigating (used when the server rejects the token). */
  clear(): void {
    this.tokenSignal.set(null);
    this.userSignal.set(null);
    removeStorage(TOKEN_KEY);
    removeStorage(USER_KEY);
  }
}

function readToken(): string | null {
  return readStorage(TOKEN_KEY);
}

function readUser(): CurrentUser | null {
  const raw = readStorage(USER_KEY);
  if (!raw) {
    return null;
  }
  try {
    return JSON.parse(raw) as CurrentUser;
  } catch {
    // A corrupt entry must not break startup.
    removeStorage(USER_KEY);
    return null;
  }
}

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Private browsing or a storage quota error: the session simply will not persist.
  }
}

function removeStorage(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    // Ignore.
  }
}
