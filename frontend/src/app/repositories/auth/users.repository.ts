import { HttpClient } from '@angular/common/http';
import { apiUrl } from 'app/core/api-url';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { CurrentUser, Permission } from 'app/core/auth/auth.service';
import { PasswordReset } from 'app/models/links';

export interface NewUser {
  /** The sign-in identifier; the CMS does not require an email address. */
  username: string;
  /** Optional contact address. */
  email: string | null;
  is_admin: boolean;
  permission: Permission;
}

/** The replacement token a password change answers with. */
export interface PasswordChanged {
  token: string;
  expires_at: string;
}

/**
 * A partial change to one account: only the fields being changed are sent, so a screen
 * that knows about roles cannot accidentally reset anything else.
 */
export interface UserChange {
  is_admin?: boolean;
  is_active?: boolean;
  permission?: Permission;
  /** Replaces the collection overrides when present; the map is sent whole. */
  collection_permissions?: Record<string, Permission>;
  single_page_permissions?: Record<string, Permission>;
}

/**
 * Account management. Administrator only, which the server enforces; `providedIn: 'root'`
 * like the other repositories so every consumer shares one instance.
 */
@Injectable({
  providedIn: 'root',
})
export class UsersRepository {
  private http = inject(HttpClient);

  list(): Observable<CurrentUser[]> {
    return this.http.get<CurrentUser[]>(apiUrl('/auth/users'));
  }

  create(user: NewUser): Observable<CurrentUser> {
    return this.http.post<CurrentUser>(apiUrl('/auth/users'), user);
  }

  update(id: string, change: UserChange): Observable<CurrentUser> {
    return this.http.patch<CurrentUser>(apiUrl(`/auth/users/${id}`), change);
  }

  remove(id: string): Observable<void> {
    return this.http.delete<void>(apiUrl(`/auth/users/${id}`));
  }

  /**
   * An administrator giving one account a new way in.
   *
   * What comes back depends on the deployment: here the CMS holds the password, so it is a link
   * the owner completes; where an identity provider holds it, the answer is a temporary password
   * the person changes at their next sign-in. `kind` says which, and `GET /auth/capabilities`
   * promised it in advance.
   */
  issuePasswordReset(id: string): Observable<PasswordReset> {
    return this.http.post<PasswordReset>(apiUrl(`/auth/users/${id}/password-reset`), null);
  }

  /**
   * Changing your own password; the current one proves it is really you.
   *
   * The answer carries a token for the new generation: the change ends every session,
   * this one included, so the caller has to adopt the replacement to stay signed in.
   */
  changeOwnPassword(currentPassword: string, newPassword: string): Observable<PasswordChanged> {
    return this.http.post<PasswordChanged>(apiUrl('/auth/me/password'), {
      current_password: currentPassword,
      new_password: newPassword,
    });
  }
}
