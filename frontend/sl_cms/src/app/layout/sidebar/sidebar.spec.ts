import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { provideRouter } from '@angular/router';

import { AuthService, CurrentUser } from 'app/core/auth/auth.service';
import { Sidebar } from './sidebar';

/** An account that may do everything, as the server would answer for an administrator. */
const ADMIN: CurrentUser = {
  id: '1',
  username: 'admin@example.com',
  email: null,
  is_admin: true,
  is_active: true,
  permission: { can_view: true, can_edit: true, can_publish: true },
  created_at: '2026-01-01T00:00:00Z',
  last_login: null,
  collection_permissions: {},
  single_page_permissions: {},
};

/**
 * Sign an account in for the next `AuthService` that is constructed.
 *
 * The service reads the stored session once, at construction, so seeding storage is enough -
 * and it is removed again as soon as the service has read it, so no other spec inherits it.
 */
function signIn(user: CurrentUser): void {
  localStorage.setItem('sl_cms.token', 'test-token');
  localStorage.setItem('sl_cms.user', JSON.stringify(user));
}

/** The link to `path`, wherever the tree keeps it. */
function hasLink(nodes: ReturnType<Sidebar['toTreeNodes']>, path: string): boolean {
  return nodes.some(
    (node) => node.link === path || (node.children ? hasLink(node.children, path) : false),
  );
}

/** The node a label names, wherever the tree keeps it. */
function nodeNamed(
  nodes: ReturnType<Sidebar['toTreeNodes']>,
  name: string,
): ReturnType<Sidebar['toTreeNodes']>[number] | null {
  for (const node of nodes) {
    if (node.name === name) {
      return node;
    }
    const under = node.children ? nodeNamed(node.children, name) : null;
    if (under) {
      return under;
    }
  }
  return null;
}

describe('Sidebar', () => {
  let component: Sidebar;
  let fixture: ComponentFixture<Sidebar>;

  async function create(): Promise<void> {
    // An administrator is a different stored session, so the module is rebuilt for it.
    TestBed.resetTestingModule();
    await TestBed.configureTestingModule({
      imports: [Sidebar],
      // Sidebar loads the collection list on init; without the testing backend this
      // would issue a real XHR that fails as an unhandled error.
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    }).compileComponents();

    fixture = TestBed.createComponent(Sidebar);
    component = fixture.componentInstance;
    await fixture.whenStable();
  }

  beforeEach(async () => {
    localStorage.removeItem('sl_cms.token');
    localStorage.removeItem('sl_cms.user');
    await create();
  });

  afterEach(() => {
    localStorage.removeItem('sl_cms.token');
    localStorage.removeItem('sl_cms.user');
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });

  it('keeps the schema and account screens out of an editor’s navigation', () => {
    TestBed.inject(AuthService);
    expect(hasLink(component.toTreeNodes([[], []]), '/settings/users')).toBe(false);
  });

  // Accounts are not a schema: what a schema describes is content, and an account is not content.
  // They were a fourth item inside the Schemas branch, which made "Schemas" and "Accounts" look
  // like the same kind of thing.
  it('keeps accounts beside the schema branch, not inside it', async () => {
    signIn(ADMIN);
    await create();
    localStorage.removeItem('sl_cms.token');
    localStorage.removeItem('sl_cms.user');

    const tree = component.toTreeNodes([[], []]);
    const schemas = nodeNamed(tree, 'Schemas');
    const settings = nodeNamed(tree, 'Settings');

    expect(schemas?.children?.map((child) => child.name)).toEqual([
      'Collections',
      'Single pages',
      'Composite fields',
    ]);
    expect(hasLink(schemas?.children ?? [], '/settings/users')).toBe(false);
    // A sibling of the schema branch, so Settings names both.
    expect(settings?.children?.map((child) => child.name)).toEqual(['Schemas', 'Accounts']);
    expect(hasLink(settings?.children ?? [], '/settings/users')).toBe(true);
  });

  it('offers the account screen to an administrator', async () => {
    // The tree reads the account when it builds, so this is the case where the profile has to
    // have arrived before the navigation is drawn.
    signIn(ADMIN);
    await create();
    // Reading the account is what consumed the stored session.
    localStorage.removeItem('sl_cms.token');
    localStorage.removeItem('sl_cms.user');

    expect(TestBed.inject(AuthService).isAdmin()).toBe(true);
    expect(hasLink(component.toTreeNodes([[], []]), '/settings/users')).toBe(true);
  });
});
