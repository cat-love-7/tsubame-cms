import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';

import {
  AuthService,
  CurrentUser,
  Permission,
  ResourceRole,
  Role,
  permissionFor,
  permissionForResource,
  resourceRoleOf,
  roleOf,
} from 'app/core/auth/auth.service';
import { errorMessage as message } from 'app/core/http-error';
import { UsersService } from 'app/services/auth/users.service';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single_pages.service';

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
  styleUrl: './list.scss',
})
export class List {
  private users = inject(UsersService);
  private auth = inject(AuthService);
  private collectionsService = inject(CollectionsService);
  private singlePages = inject(SinglePagesService);

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

  /** The resources an override can be given for. */
  public collections: string[] = [];
  public pages: string[] = [];

  /** The account whose resource permissions are open, if any. */
  public editingResources = signal<string | null>(null);
  /** What the selects currently show, keyed by resource name. */
  public draftCollections: Record<string, ResourceRole> = {};
  public draftPages: Record<string, ResourceRole> = {};

  public readonly resourceRoles: { value: ResourceRole; label: string }[] = [
    { value: 'inherit', label: '共通のロール' },
    { value: 'viewer', label: '確認(閲覧のみ)' },
    { value: 'editor', label: '編集(下書きまで)' },
    { value: 'publisher', label: '公開(編集 + 公開)' },
    { value: 'deny', label: 'なし(拒否)' },
  ];

  constructor() {
    this.load();
    this.collectionsService.getAllCollectionNames().subscribe({
      next: (names) => (this.collections = names),
      error: (e) => this.error.set(`Failed to load the collections: ${message(e)}`),
    });
    this.singlePages.listPageNames().subscribe({
      next: (names) => (this.pages = names),
      error: (e) => this.error.set(`Failed to load the single pages: ${message(e)}`),
    });
  }

  /** Open the per-resource editor for one account, seeded from what is stored. */
  openResources(user: CurrentUser) {
    this.error.set('');
    this.status.set('');
    this.draftCollections = {};
    this.draftPages = {};
    for (const name of this.collections) {
      this.draftCollections[name] = resourceRoleOf(user.collection_permissions?.[name]);
    }
    for (const name of this.pages) {
      this.draftPages[name] = resourceRoleOf(user.single_page_permissions?.[name]);
    }
    this.editingResources.set(user.id);
  }

  /** A one-line summary of the overrides, for the account row. */
  resourceSummary(user: CurrentUser): string {
    const parts = [
      ...Object.entries(user.collection_permissions ?? {}),
      ...Object.entries(user.single_page_permissions ?? {}),
    ]
      .filter(([, permission]) => permission !== undefined)
      .map(([name, permission]) => `${name}=${this.labelOf(resourceRoleOf(permission))}`);
    return parts.length === 0 ? '—' : parts.join(', ');
  }

  labelOf(role: ResourceRole): string {
    return this.resourceRoles.find((option) => option.value === role)?.label ?? role;
  }

  /** Store the overrides for one account; only entries that differ are sent. */
  saveResources(user: CurrentUser) {
    this.error.set('');
    this.status.set('');
    const collections: Record<string, Permission> = {};
    for (const [name, role] of Object.entries(this.draftCollections)) {
      const permission = permissionForResource(role);
      if (permission) {
        collections[name] = permission;
      }
    }
    const pages: Record<string, Permission> = {};
    for (const [name, role] of Object.entries(this.draftPages)) {
      const permission = permissionForResource(role);
      if (permission) {
        pages[name] = permission;
      }
    }

    this.users
      .update(user.id, {
        collection_permissions: collections,
        single_page_permissions: pages,
      })
      .subscribe({
        next: () => {
          this.status.set(`${user.email} のリソース権限を保存しました`);
          this.editingResources.set(null);
          this.load();
        },
        error: (e) => this.error.set(`Could not save the permissions: ${message(e)}`),
      });
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
