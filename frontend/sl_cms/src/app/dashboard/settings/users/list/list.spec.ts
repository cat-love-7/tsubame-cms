import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { of } from 'rxjs';

import { AuthService, CurrentUser, Permission } from 'app/core/auth/auth.service';
import { t } from 'app/core/i18n/message';
import { UsersService } from 'app/services/auth/users.service';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single-pages.service';

import { UsersList } from './list';

function account(overrides: Partial<CurrentUser> = {}): CurrentUser {
  return {
    id: 'user-1',
    username: 'editor@example.com',
    email: null,
    is_admin: false,
    is_active: true,
    permission: { can_view: true, can_edit: true, can_publish: false },
    created_at: '2024-01-01T00:00:00Z',
    last_login: null,
    collection_permissions: {},
    single_page_permissions: {},
    ...overrides,
  };
}

/** A fake account store, so role changes can be observed without HTTP. */
class StubUsersService {
  public accounts: CurrentUser[] = [account()];
  public created: unknown[] = [];
  public updated: { id: string; change: unknown }[] = [];
  public removed: string[] = [];
  public reset: string[] = [];

  list = () => of(this.accounts);
  create = (user: unknown) => {
    this.created.push(user);
    return of(account());
  };
  update = (id: string, change: unknown) => {
    this.updated.push({ id, change });
    return of(account());
  };
  remove = (id: string) => {
    this.removed.push(id);
    return of(void 0);
  };
  changeOwnPassword = () => of({ token: 'replacement-token', expires_at: '2024-01-01T00:00:00Z' });
  issuePasswordResetLink = (id: string) => {
    this.resetIssued.push(id);
    return of({ token: 'user-1.0.1758000000.abc123', expires_at: '2026-09-13T12:00:00Z' });
  };
  public resetIssued: string[] = [];
}

describe('Accounts', () => {
  let fixture: ComponentFixture<UsersList>;
  let stub: StubUsersService;

  beforeEach(async () => {
    stub = new StubUsersService();
    await TestBed.configureTestingModule({
      imports: [UsersList],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: UsersService, useValue: stub },
        { provide: AuthService, useValue: { user: () => null, isAdmin: () => true } },
        {
          provide: CollectionsService,
          useValue: { getAllCollectionNames: () => of(['blog', 'news']) },
        },
        { provide: SinglePagesService, useValue: { listPageNames: () => of(['home']) } },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(UsersList);
    await fixture.whenStable();
    fixture.detectChanges();
  });

  /** A per-resource grant is edited here and stored as the capabilities it stands for. */
  it('saves a per-collection override and leaves the rest inherited', () => {
    const user = stub.accounts[0];
    fixture.componentInstance.openResources(user);
    fixture.detectChanges();

    // The panel lists the resources with what is stored today.
    expect(fixture.nativeElement.querySelectorAll('.resource').length).toBe(3);
    expect(fixture.nativeElement.textContent).toContain('collection: blog');

    fixture.componentInstance.draftCollections['blog'] = 'editor';
    fixture.componentInstance.draftCollections['news'] = 'deny';
    fixture.componentInstance.draftPages['home'] = 'inherit';
    fixture.componentInstance.saveResources(user);

    expect(stub.updated.length).toBe(1);
    const change = stub.updated[0].change as {
      collection_permissions: Record<string, { can_edit: boolean; can_publish: boolean }>;
      single_page_permissions: Record<string, unknown>;
    };
    // Only the resources that differ from "共通のロール" are sent.
    expect(Object.keys(change.collection_permissions)).toEqual(['blog', 'news']);
    expect(change.collection_permissions['blog'].can_edit).toBe(true);
    expect(change.collection_permissions['blog'].can_publish).toBe(false);
    expect(change.collection_permissions['news'].can_edit).toBe(false);
    expect(change.single_page_permissions).toEqual({});
  });

  /** The administrator gets a link to hand on; the CMS mails nothing. */
  it('issues a password reset link and shows it', () => {
    fixture.componentInstance.issuePasswordResetLink(stub.accounts[0]);

    expect(stub.resetIssued).toEqual(['user-1']);
    expect(fixture.componentInstance.resetLink()).toBe(
      `${location.origin}/reset-password?token=user-1.0.1758000000.abc123`,
    );
    expect(fixture.componentInstance.resetFor()).toBe('editor@example.com');
  });

  it("lists an account's overrides in its row", () => {
    const user = account({
      collection_permissions: { blog: { can_view: true, can_edit: true, can_publish: false } },
      single_page_permissions: { home: { can_view: false, can_edit: false, can_publish: false } },
    });
    expect(fixture.componentInstance.resourceSummary(user)).toContain('blog=');
    expect(fixture.componentInstance.resourceSummary(user)).toContain('home=');
    expect(fixture.componentInstance.resourceSummary(stub.accounts[0])).toBe('—');
  });

  it('lists the accounts with the role they hold', () => {
    expect(fixture.nativeElement.textContent).toContain('editor@example.com');
    expect(fixture.componentInstance.roleOf(stub.accounts[0])).toBe('editor');
  });

  /** The screen stores roles; the server stores the capabilities they expand to. */
  it('changes a role into the matching capabilities', () => {
    fixture.componentInstance.setRole(stub.accounts[0], 'publisher');

    expect(stub.updated).toEqual([
      {
        id: 'user-1',
        change: { permission: { can_view: true, can_edit: true, can_publish: true } },
      },
    ]);
  });

  // No password is chosen here: the account has no credential until its owner follows the reset
  // link this screen hands over, which is the only path that may set one.
  it('creates an account with the chosen role, and offers a reset link for it', () => {
    fixture.componentInstance.newUsername = 'new-ops';
    fixture.componentInstance.newRole = 'viewer';
    fixture.componentInstance.create();

    // No address: the CMS does not need one.
    expect(stub.created).toEqual([
      {
        username: 'new-ops',
        email: null,
        is_admin: false,
        permission: { can_view: true, can_edit: false, can_publish: false },
      },
    ]);
    expect(stub.resetIssued).toEqual(['user-1']);
    expect(fixture.componentInstance.resetLink()).toContain('/reset-password?token=');
  });

  /** An address, when the operator records one, is passed on as contact data. */
  it('records an optional contact address when one is given', () => {
    fixture.componentInstance.newUsername = 'new-ops';
    fixture.componentInstance.newEmail = ' ops@example.com ';
    fixture.componentInstance.create();

    expect((stub.created[0] as { email: string | null }).email).toBe('ops@example.com');
  });

  // What it says it did has to name the account. It used `email`, which is optional and usually
  // absent, so these read "Changed the administrator flag of ." and "Enabled ." - and the
  // identifier an account is known by is its username.
  it('names the account in what it says it did', () => {
    const fresh = TestBed.createComponent(UsersList);
    fresh.detectChanges();

    fresh.componentInstance.setAdmin(stub.accounts[0], true);
    expect(fresh.componentInstance.status()).toEqual(
      t('accounts.adminChanged', { user: 'editor@example.com' }),
    );

    fresh.componentInstance.setActive(stub.accounts[0], false);
    expect(fresh.componentInstance.status()).toEqual(
      t('accounts.deactivated', { user: 'editor@example.com' }),
    );
  });

  it('disables an account rather than deleting it', () => {
    fixture.componentInstance.setActive(stub.accounts[0], false);

    expect(stub.updated).toEqual([{ id: 'user-1', change: { is_active: false } }]);
  });

  it('deletes an account after confirming', () => {
    vi.spyOn(window, 'confirm').mockReturnValue(true);

    fixture.componentInstance.remove(stub.accounts[0]);

    expect(stub.removed).toEqual(['user-1']);
  });
});
