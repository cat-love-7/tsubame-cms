import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { of } from 'rxjs';

import { AuthService, CurrentUser, Permission } from 'app/core/auth/auth.service';
import { UsersService } from 'app/services/auth/users.service';

import { List } from './list';

function account(overrides: Partial<CurrentUser> = {}): CurrentUser {
  return {
    id: 'user-1',
    email: 'editor@example.com',
    is_admin: false,
    is_active: true,
    permission: { can_view: true, can_edit: true, can_publish: false },
    created_at: '2024-01-01T00:00:00Z',
    last_login: null,
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
  resetPassword = (id: string) => {
    this.reset.push(id);
    return of(void 0);
  };
  changeOwnPassword = () => of({ token: 'replacement-token', expires_at: '2024-01-01T00:00:00Z' });
}

describe('Accounts', () => {
  let fixture: ComponentFixture<List>;
  let stub: StubUsersService;

  beforeEach(async () => {
    stub = new StubUsersService();
    await TestBed.configureTestingModule({
      imports: [List],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: UsersService, useValue: stub },
        { provide: AuthService, useValue: { user: () => null, isAdmin: () => true } },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(List);
    await fixture.whenStable();
    fixture.detectChanges();
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

  it('creates an account with the chosen role', () => {
    fixture.componentInstance.newEmail = 'new@example.com';
    fixture.componentInstance.newPassword = 'user-password';
    fixture.componentInstance.newRole = 'viewer';
    fixture.componentInstance.create();

    expect(stub.created).toEqual([
      {
        email: 'new@example.com',
        password: 'user-password',
        is_admin: false,
        permission: { can_view: true, can_edit: false, can_publish: false },
      },
    ]);
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

  it('resets a password with the one that was typed', () => {
    vi.spyOn(window, 'prompt').mockReturnValue('reset-password');

    fixture.componentInstance.resetPassword(stub.accounts[0]);

    expect(stub.reset).toEqual(['user-1']);
  });
});
