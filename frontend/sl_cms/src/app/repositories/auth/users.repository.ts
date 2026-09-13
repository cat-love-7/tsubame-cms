import { HttpClient } from '@angular/common/http';
import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';

import { CurrentUser, Permission } from 'app/core/auth/auth.service';
import { PasswordResetLink } from 'app/models/item-status';

export interface NewUser {
  /** The sign-in identifier; the CMS does not require an email address. */
  username: string;
  password: string;
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
  constructor(private http: HttpClient) {}

  list(): Observable<CurrentUser[]> {
    return this.http.get<CurrentUser[]>('/api/auth/users');
  }

  create(user: NewUser): Observable<CurrentUser> {
    return this.http.post<CurrentUser>('/api/auth/users', user);
  }

  update(id: string, change: UserChange): Observable<CurrentUser> {
    return this.http.patch<CurrentUser>(`/api/auth/users/${id}`, change);
  }

  remove(id: string): Observable<void> {
    return this.http.delete<void>(`/api/auth/users/${id}`);
  }

  /**
   * An administrator issuing a link that lets one account set its own new password.
   *
   * Nothing is mailed: the caller shows the link so it can be passed on, which is also the only
   * thing that works for an account with no address on file.
   */
  issuePasswordResetLink(id: string): Observable<PasswordResetLink> {
    return this.http.post<PasswordResetLink>(`/api/auth/users/${id}/password-reset-link`, null);
  }

  /** An administrator setting someone else's password. */
  resetPassword(id: string, password: string): Observable<void> {
    return this.http.post<void>(`/api/auth/users/${id}/password`, { password });
  }

  /**
   * Changing your own password; the current one proves it is really you.
   *
   * The answer carries a token for the new generation: the change ends every session,
   * this one included, so the caller has to adopt the replacement to stay signed in.
   */
  changeOwnPassword(currentPassword: string, newPassword: string): Observable<PasswordChanged> {
    return this.http.post<PasswordChanged>('/api/auth/me/password', {
      current_password: currentPassword,
      new_password: newPassword,
    });
  }
}
