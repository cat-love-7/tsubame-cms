import { HttpClient } from '@angular/common/http';
import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';

import { CurrentUser, Permission } from 'app/core/auth/auth.service';

export interface NewUser {
  email: string;
  password: string;
  is_admin: boolean;
  permission: Permission;
}

/**
 * A partial change to one account: only the fields being changed are sent, so a screen
 * that knows about roles cannot accidentally reset anything else.
 */
export interface UserChange {
  is_admin?: boolean;
  is_active?: boolean;
  permission?: Permission;
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

  /** An administrator setting someone else's password. */
  resetPassword(id: string, password: string): Observable<void> {
    return this.http.post<void>(`/api/auth/users/${id}/password`, { password });
  }

  /** Changing your own password; the current one proves it is really you. */
  changeOwnPassword(currentPassword: string, newPassword: string): Observable<void> {
    return this.http.post<void>('/api/auth/me/password', {
      current_password: currentPassword,
      new_password: newPassword,
    });
  }
}
