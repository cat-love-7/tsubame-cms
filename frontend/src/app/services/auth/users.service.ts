import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { CurrentUser } from 'app/core/auth/auth.service';
import { PasswordReset } from 'app/models/links';

import {
  NewUser,
  PasswordChanged,
  UserChange,
  UsersRepository,
} from 'app/repositories/auth/users.repository';

@Injectable({
  providedIn: 'root',
})
export class UsersService {
  private users = inject(UsersRepository);

  list(): Observable<CurrentUser[]> {
    return this.users.list();
  }

  create(user: NewUser): Observable<CurrentUser> {
    return this.users.create(user);
  }

  update(id: string, change: UserChange): Observable<CurrentUser> {
    return this.users.update(id, change);
  }

  remove(id: string): Observable<void> {
    return this.users.remove(id);
  }

  issuePasswordReset(id: string): Observable<PasswordReset> {
    return this.users.issuePasswordReset(id);
  }

  changeOwnPassword(currentPassword: string, newPassword: string): Observable<PasswordChanged> {
    return this.users.changeOwnPassword(currentPassword, newPassword);
  }
}
