import { Component, computed, inject, signal, ChangeDetectionStrategy } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';

import { AuthService, CurrentUser, Permission, Role, permissionFor, roleOf } from 'app/core/auth/auth.service';
import { errorMessage as message } from 'app/core/http-error';
import { UsersService } from 'app/services/auth/users.service';

interface RoleOption {
  value: Role;
  label: string;
  hint: string;
}

/**
 * Account management, for administrators.
 *
 * Roles are presented as the three things a CMS needs — read and confirm, prepare changes,
 * or release them — while the server stores the capabilities they expand to. Administrator
 * is a separate switch, because managing accounts is not the same as editing content.
 */
@Component({
  selector: 'app-users-list',
  imports: [
    FormsModule,
    MatButtonModule,
    MatCheckboxModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatSelectModule,
  ],
  templateUrl: './list.html',
  changeDetection: ChangeDetectionStrategy.Eager,
  styleUrl: './list.scss',
})
export class List {
  private users = inject(UsersService);
  private auth = inject(AuthService);

  public accounts = signal<CurrentUser[]>([]);
  public error = signal('');
  public status = signal('');

  /** The signed-in administrator, so the screen can refuse to lock itself out. */
  public me = computed(() => this.auth.user());

  public newEmail = '';
  public newPassword = '';
  public newRole: Role = 'viewer';
  public newIsAdmin = false;

  public readonly roles: RoleOption[] = [
    { value: 'viewer', label: '確認(閲覧のみ)', hint: '下書きを含めて読めますが、編集はできません' },
    { value: 'editor', label: '編集(下書きまで)', hint: '保存しても、公開するまでサイトには出ません' },
    { value: 'publisher', label: '公開(編集 + 公開)', hint: '公開・非公開、削除もできます' },
  ];

  constructor() {
    this.load();
  }

  private load() {
    this.users.list().subscribe({
      next: (accounts) => this.accounts.set(accounts),
      error: (e) => this.error.set(`Failed to load the accounts: ${message(e)}`),
    });
  }

  roleOf(user: CurrentUser): Role {
    return roleOf(user);
  }

  create() {
    this.error.set('');
    this.status.set('');
    this.users
      .create({
        email: this.newEmail,
        password: this.newPassword,
        is_admin: this.newIsAdmin,
        permission: this.newIsAdmin ? permissionFor('publisher') : permissionFor(this.newRole),
      })
      .subscribe({
        next: () => {
          this.newEmail = '';
          this.newPassword = '';
          this.newRole = 'viewer';
          this.newIsAdmin = false;
          this.status.set('アカウントを作成しました');
          this.load();
        },
        error: (e) => this.error.set(`Could not create the account: ${message(e)}`),
      });
  }

  setRole(user: CurrentUser, role: Role) {
    this.change(user, { permission: permissionFor(role) }, `${user.email} のロールを変更しました`);
  }

  setAdmin(user: CurrentUser, isAdmin: boolean) {
    this.change(user, { is_admin: isAdmin }, `${user.email} の管理者権限を変更しました`);
  }

  setActive(user: CurrentUser, isActive: boolean) {
    this.change(
      user,
      { is_active: isActive },
      isActive ? `${user.email} を有効にしました` : `${user.email} を無効にしました`,
    );
  }

  private change(user: CurrentUser, change: Parameters<UsersService['update']>[1], note: string) {
    this.error.set('');
    this.status.set('');
    this.users.update(user.id, change).subscribe({
      next: () => {
        this.status.set(note);
        this.load();
      },
      error: (e) => {
        // The server refuses changes that would leave nobody able to manage the CMS.
        this.error.set(`Could not change ${user.email}: ${message(e)}`);
        this.load();
      },
    });
  }

  resetPassword(user: CurrentUser) {
    const password = prompt(`${user.email} の新しいパスワード(8 文字以上)`);
    if (!password) {
      return;
    }
    this.error.set('');
    this.status.set('');
    this.users.resetPassword(user.id, password).subscribe({
      next: () => this.status.set(`${user.email} のパスワードを再設定しました`),
      error: (e) => this.error.set(`Could not reset the password: ${message(e)}`),
    });
  }

  remove(user: CurrentUser) {
    if (!confirm(`${user.email} を削除しますか?`)) {
      return;
    }
    this.error.set('');
    this.status.set('');
    this.users.remove(user.id).subscribe({
      next: () => {
        this.status.set(`${user.email} を削除しました`);
        this.load();
      },
      error: (e) => this.error.set(`Could not delete the account: ${message(e)}`),
    });
  }

  /** Only the last active administrator is protected, and only the server knows that. */
  isMe(user: CurrentUser): boolean {
    return this.me()?.id === user.id;
  }
}
