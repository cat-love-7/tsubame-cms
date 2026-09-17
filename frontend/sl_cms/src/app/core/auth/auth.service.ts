import { HttpClient } from '@angular/common/http';
import { computed, inject, Injectable, signal } from '@angular/core';
import { Router } from '@angular/router';
import { Observable, map, of, switchMap, tap } from 'rxjs';

export interface Permission {
  can_publish: boolean;
  can_edit: boolean;
  can_view: boolean;
}

export interface CurrentUser {
  id: string;
  /** The sign-in identifier, and the only name the CMS requires. */
  username: string;
  /** Contact address, when the operator recorded one. */
  email: string | null;
  is_admin: boolean;
  /** False for an account that has been disabled; it also cannot sign in. */
  is_active: boolean;
  permission: Permission;
  created_at: string;
  last_login: string | null;
  /** Per-resource overrides; absent name means the account-wide permission applies. */
  collection_permissions: Record<string, Permission>;
  single_page_permissions: Record<string, Permission>;
}

/** The role options a resource override can take, including "the account-wide one" and "none". */
export type ResourceRole = Role | 'inherit' | 'deny';

/** Which role an override holds, reading the stored capabilities back. */
export function resourceRoleOf(permission: Permission | undefined): ResourceRole {
  if (!permission) {
    return 'inherit';
  }
  if (!permission.can_view && !permission.can_edit && !permission.can_publish) {
    return 'deny';
  }
  return roleOfPermission(permission);
}

/** The capabilities a resource override stands for. */
export function permissionForResource(role: ResourceRole): Permission | null {
  if (role === 'inherit') {
    return null;
  }
  if (role === 'deny') {
    return { can_view: false, can_edit: false, can_publish: false };
  }
  return permissionFor(role);
}

/** The kinds of resource that can carry permissions of their own. */
export type ResourceKind = 'collections' | 'single_pages';

/** The three capabilities, presented as roles on the account screens. */
export type Role = 'viewer' | 'editor' | 'publisher';

export function roleOf(user: CurrentUser): Role {
  return roleOfPermission(user.permission);
}

/** The role a set of capabilities reads as. */
export function roleOfPermission(permission: Permission): Role {
  if (!permission.can_edit) {
    return 'viewer';
  }
  return permission.can_publish ? 'publisher' : 'editor';
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

/** What a password change (or a completed reset) answers with. */
interface PasswordChanged {
  token: string;
  expires_at: string;
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

  /**
   * Whether this account may add an image to the library.
   *
   * An image belongs to no collection or page, so there is no resource to judge: anyone who may
   * edit *something* needs the images that something uses. Changing or deleting an image that
   * other content may be using stays with {@link canEdit}, which is the account-wide permission -
   * the server draws the same line.
   */
  readonly canUploadImages = computed(() => {
    const user = this.userSignal();
    if (user === null) {
      return false;
    }
    return (
      user.is_admin ||
      user.permission.can_edit ||
      Object.values(user.collection_permissions ?? {}).some((permission) => permission.can_edit) ||
      Object.values(user.single_page_permissions ?? {}).some((permission) => permission.can_edit)
    );
  });
  readonly isAdmin = computed(() => this.userSignal()?.is_admin ?? false);

  token(): string | null {
    return this.tokenSignal();
  }

  /**
   * Whether the signed-in account may read / edit / release one named resource.
   *
   * A per-resource override replaces the account-wide role for that resource (see the
   * server's `User::permission_for_collection`), so the screens have to ask about the
   * resource they are showing rather than about the account in general. An administrator is
   * allowed everywhere.
   */
  canReadIn(kind: ResourceKind, name: string): boolean {
    return this.allowedIn(kind, name, (permission) => permission.can_view);
  }

  canEditIn(kind: ResourceKind, name: string): boolean {
    return this.allowedIn(kind, name, (permission) => permission.can_edit);
  }

  canPublishIn(kind: ResourceKind, name: string): boolean {
    return this.allowedIn(kind, name, (permission) => permission.can_publish);
  }

  private allowedIn(
    kind: ResourceKind,
    name: string,
    allows: (permission: Permission) => boolean,
  ): boolean {
    const user = this.userSignal();
    if (!user) {
      return false;
    }
    if (user.is_admin) {
      return true;
    }
    const overrides =
      kind === 'collections' ? user.collection_permissions : user.single_page_permissions;
    return allows(overrides?.[name] ?? user.permission);
  }

  login(username: string, password: string): Observable<LoginResponse> {
    return this.http.post<LoginResponse>('/api/auth/login', { username, password }).pipe(
      tap((response) => {
        this.tokenSignal.set(response.token);
        this.userSignal.set(response.user);
        writeStorage(TOKEN_KEY, response.token);
        writeStorage(USER_KEY, JSON.stringify(response.user));
      }),
    );
  }

  /**
   * Set a new password with an administrator-issued link, and sign in with the result.
   *
   * The link is the credential, so this needs no session; completing it ends whatever sessions
   * existed, which is why the answer carries a token for the caller to adopt. The account's own
   * record is then read: the person following the link was signed out (that is usually why they
   * needed it), so the token would otherwise be all the CMS knows about them - no name, no
   * permissions, and no way to fill either in short of signing in again.
   */
  completePasswordReset(token: string, newPassword: string): Observable<PasswordChanged> {
    return this.http
      .post<PasswordChanged>('/api/auth/password-reset', { token, new_password: newPassword })
      .pipe(
        tap((changed) => this.replaceToken(changed.token)),
        switchMap((changed) => this.loadUser().pipe(map(() => changed))),
      );
  }

  /**
   * Read the signed-in account from the server and remember it.
   *
   * Used wherever the CMS holds a token but not (yet) the record it stands for: a page loaded
   * with a token from storage and nothing beside it, and the moment after a password reset.
   */
  loadUser(): Observable<CurrentUser> {
    return this.http.get<CurrentUser>('/api/auth/me').pipe(
      tap((user) => {
        this.userSignal.set(user);
        writeStorage(USER_KEY, JSON.stringify(user));
      }),
    );
  }

  /**
   * Fill in the account when a token is remembered but the record beside it is not.
   *
   * A CMS that knows who is signed in only by token would offer the screens of a viewer: every
   * permission question is answered from the record. Nothing is fetched when the record is
   * already there, so this costs nothing on an ordinary load.
   */
  loadUserIfMissing(): Observable<CurrentUser | null> {
    if (this.tokenSignal() === null || this.userSignal() !== null) {
      return of(null);
    }
    return this.loadUser();
  }

  /**
   * Adopt a token issued in place of the current one.
   *
   * A password change ends every session, so the replacement the server returns has to
   * take the old token's place or the next request would be refused.
   */
  replaceToken(token: string): void {
    this.tokenSignal.set(token);
    writeStorage(TOKEN_KEY, token);
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
