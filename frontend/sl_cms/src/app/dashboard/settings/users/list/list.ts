import { Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';

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
import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe, failure, t } from 'app/core/i18n/message';
import { UsersService } from 'app/services/auth/users.service';
import { copyToClipboard, passwordResetUrl } from 'app/shared/share-link';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single_pages.service';

interface RoleOption {
  value: Role;
  /** The catalog key of the role's name, and of the one line under it. */
  labelKey: string;
  hintKey: string;
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
    MessagePipe,
    TranslocoPipe,
  ],
  templateUrl: './list.html',
  styleUrl: './list.scss',
})
export class List {

  private users = inject(UsersService);
  private auth = inject(AuthService);
  private collectionsService = inject(CollectionsService);
  private singlePages = inject(SinglePagesService);
  private capabilities = inject(CapabilitiesService);
  private i18n = inject(TranslocoService);
  private dates = inject(DateTimeFormat);

  /**
   * Whether this deployment owns the passwords. Where it does not, the account is created and
   * its password reset at the identity provider, so those controls are not this CMS's to offer.
   */
  public passwordLogin = this.capabilities.passwordLogin;
  public passwordResetLinks = this.capabilities.passwordResetLinks;

  public accounts = signal<CurrentUser[]>([]);
  /** A failure or a note about what just happened: keys, so they follow a language change. */
  public error = signal<Message | null>(null);
  public status = signal<Message | null>(null);

  /** The signed-in administrator, so the screen can refuse to lock itself out. */
  public me = computed(() => this.auth.user());

  public newUsername = '';
  /** Optional: the CMS works without an address, but an operator may want one on file. */
  public newEmail = '';
  public newPassword = '';
  public newRole: Role = 'viewer';
  public newIsAdmin = false;

  public readonly roles: RoleOption[] = [
    { value: 'viewer', labelKey: 'roles.viewer', hintKey: 'roles.viewerHint' },
    { value: 'editor', labelKey: 'roles.editor', hintKey: 'roles.editorHint' },
    { value: 'publisher', labelKey: 'roles.publisher', hintKey: 'roles.publisherHint' },
  ];

  /** The resources an override can be given for. */
  public collections: string[] = [];
  public pages: string[] = [];

  /** The account whose resource permissions are open, if any. */
  public editingResources = signal<string | null>(null);
  /** What the selects currently show, keyed by resource name. */
  public draftCollections: Record<string, ResourceRole> = {};
  public draftPages: Record<string, ResourceRole> = {};

  public readonly resourceRoles: { value: ResourceRole; labelKey: string }[] = [
    { value: 'inherit', labelKey: 'roles.inherit' },
    { value: 'viewer', labelKey: 'roles.viewer' },
    { value: 'editor', labelKey: 'roles.editor' },
    { value: 'publisher', labelKey: 'roles.publisher' },
    { value: 'deny', labelKey: 'roles.deny' },
  ];

  constructor() {
    this.capabilities.load();
    this.load();
    this.collectionsService.getAllCollectionNames().subscribe({
      next: (names) => (this.collections = names),
      error: (e) => this.error.set(failure('accounts.failedToLoadCollections', e)),
    });
    this.singlePages.listPageNames().subscribe({
      next: (names) => (this.pages = names),
      error: (e) => this.error.set(failure('accounts.failedToLoadSinglePages', e)),
    });
  }

  /** Open the per-resource editor for one account, seeded from what is stored. */
  openResources(user: CurrentUser) {
    this.error.set(null);
    this.status.set(null);
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
    const option = this.resourceRoles.find((candidate) => candidate.value === role);
    // Read at render time, so the summary follows a language change with the rest of the row.
    return option ? this.i18n.translate(option.labelKey) : role;
  }

  /** The reset link that was issued last, so an administrator can copy it. */
  public resetLink = signal('');
  /** The account the shown link belongs to. */
  public resetFor = signal('');

  /**
   * Issue a link that lets one account set its own new password, and offer it for copying.
   *
   * The CMS sends no mail, so the link travels however the administrator likes - which also
   * works for an account with no address on file.
   */
  issuePasswordResetLink(user: CurrentUser) {
    this.error.set(null);
    this.status.set(null);
    this.users.issuePasswordResetLink(user.id).subscribe({
      next: async (link) => {
        const url = passwordResetUrl(link.token);
        this.resetLink.set(url);
        this.resetFor.set(user.username);
        const copied = await copyToClipboard(url);
        const expires = this.dates.format(link.expires_at);
        this.status.set(
          copied
            ? t('accounts.resetLinkCopied', { user: user.username, expires })
            : t('accounts.resetLinkNotCopied', { user: user.username, expires }),
        );
      },
      error: (e) => this.error.set(failure('accounts.issueResetLinkFailed', e)),
    });
  }

  /** Store the overrides for one account; only entries that differ are sent. */
  saveResources(user: CurrentUser) {
    this.error.set(null);
    this.status.set(null);
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
          this.status.set(t('accounts.resourcesSaved', { user: user.username }));
          this.editingResources.set(null);
          this.load();
        },
        error: (e) => this.error.set(failure('accounts.savePermissionsFailed', e)),
      });
  }

  private load() {
    this.users.list().subscribe({
      next: (accounts) => this.accounts.set(accounts),
      error: (e) => this.error.set(failure('accounts.failedToLoadAccounts', e)),
    });
  }

  roleOf(user: CurrentUser): Role {
    return roleOf(user);
  }

  create() {
    this.error.set(null);
    this.status.set(null);
    this.users
      .create({
        username: this.newUsername,
        password: this.newPassword,
        // Blank means "no address on file" rather than an empty one.
        email: this.newEmail.trim() === '' ? null : this.newEmail.trim(),
        is_admin: this.newIsAdmin,
        permission: this.newIsAdmin ? permissionFor('publisher') : permissionFor(this.newRole),
      })
      .subscribe({
        next: () => {
          this.newUsername = '';
          this.newEmail = '';
          this.newPassword = '';
          this.newRole = 'viewer';
          this.newIsAdmin = false;
          this.status.set(t('accounts.created'));
          this.load();
        },
        error: (e) => this.error.set(failure('accounts.createFailed', e)),
      });
  }

  setRole(user: CurrentUser, role: Role) {
    this.change(
      user,
      { permission: permissionFor(role) },
      t('accounts.roleChanged', { user: user.username }),
    );
  }

  setAdmin(user: CurrentUser, isAdmin: boolean) {
    this.change(
      user,
      { is_admin: isAdmin },
      t('accounts.adminChanged', { user: user.username }),
    );
  }

  setActive(user: CurrentUser, isActive: boolean) {
    this.change(
      user,
      { is_active: isActive },
      t(isActive ? 'accounts.activated' : 'accounts.deactivated', { user: user.username }),
    );
  }

  private change(user: CurrentUser, change: Parameters<UsersService['update']>[1], note: Message) {
    this.error.set(null);
    this.status.set(null);
    this.users.update(user.id, change).subscribe({
      next: () => {
        this.status.set(note);
        this.load();
      },
      error: (e) => {
        // The server refuses changes that would leave nobody able to manage the CMS.
        this.error.set(failure('accounts.changeFailed', e, { user: user.username }));
        this.load();
      },
    });
  }

  resetPassword(user: CurrentUser) {
    const password = prompt(this.i18n.translate('accounts.newPasswordPrompt', { user: user.username }));
    if (!password) {
      return;
    }
    this.error.set(null);
    this.status.set(null);
    this.users.resetPassword(user.id, password).subscribe({
      next: () => this.status.set(t('accounts.passwordReset', { user: user.username })),
      error: (e) => this.error.set(failure('accounts.resetPasswordFailed', e)),
    });
  }

  remove(user: CurrentUser) {
    if (!confirm(this.i18n.translate('accounts.deleteConfirm', { user: user.username }))) {
      return;
    }
    this.error.set(null);
    this.status.set(null);
    this.users.remove(user.id).subscribe({
      next: () => {
        this.status.set(t('accounts.deleted', { user: user.username }));
        this.load();
      },
      error: (e) => this.error.set(failure('accounts.deleteFailed', e)),
    });
  }

  /** Only the last active administrator is protected, and only the server knows that. */
  isMe(user: CurrentUser): boolean {
    return this.me()?.id === user.id;
  }

  /** When the account last signed in, in the reader's language; "never" is a dash. */
  lastSignIn(user: CurrentUser): string {
    return this.dates.format(user.last_login);
  }
}
