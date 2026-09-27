# Admin UI E2E Checks

Drive the admin UI in a real browser and confirm it works **against the real API**.

`npm test` (the Vitest component tests) stubs the services, so the DOM and the logic are visible but
the shape of the traffic is not. That gap is what this check catches, and the following 4 defects were in
fact found by it.

- Fetching the list, the statuses and the navigation at the same time returns 500 (an LMDB transaction constraint)
- The mutating APIs return their body as text, so Angular fails to parse it and wrongly reports "save/delete failed"
- Holding the list state in a normal field caused `ExpressionChangedAfterItHasBeenCheckedError`, and the
  last row of the final page rendered blank
- Concurrent requests return 500 (rkv calls `mdb_dbi_open` **every time it opens the store**, and LMDB
  refuses that while another transaction is running). The read paths were fixed with a storage lock, but
  **only the password verification was left outside the lock**. A sign-in right after a password reset
  returned 500 when it overlapped with the reset screen loading the dashboard

## Prerequisites

1. Backend: `cargo run --manifest-path backend/Cargo.toml` (127.0.0.1:8080, data under
   `DATA_ROOT`. If unset, `./data` in the directory it was started from). `--manifest-path` points at the
   workspace, so the `default-members` setting starts the on-premises binary (`tsubame`).
   `scripts/test-e2e.sh` starts it with `DATA_ROOT` and `JWT_SECRET` passed in.
2. Dev server: `cd frontend && npm start` (localhost:4200, proxies `/api` to the backend)
3. Chromium (first time only): `npx playwright install chromium`

The browser goes into `~/.cache/ms-playwright` by default. **This development environment cannot
write to the home directory**, so install it with an explicit location.

```bash
export PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers
npx playwright install chromium
# Chromium needs the system libraries it links against. A freshly built container
# usually lacks them, and the browser fails with "error while loading shared
# libraries: libglib-2.0.so.0". This one needs root (apt).
npx playwright install-deps chromium
```

The same `PLAYWRIGHT_BROWSERS_PATH` is needed at run time (if it is unset, Playwright looks in
`~/.cache/ms-playwright` and fails with "Executable doesn't exist"). On a normal development machine this setting is not needed.

## Running

```bash
cd frontend
PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers npm run e2e
```

It can be adjusted with environment variables.

| Variable | Default | Meaning |
|---|---|---|
| `BASE_URL` | `http://localhost:4200` | Dev server |
| `ADMIN_USERNAME` / `ADMIN_PASSWORD` | `admin@example.com` / `admin-password` | Administrator ID used for seeding and sign-in (the server also accepts `ADMIN_EMAIL`) |
| `COLLECTION` | `e2e_blog` | Collection used to check paging |
| `TOTAL` | `60` | Its item count (51 or more is required because there is a 50-item page check) |
| `LAST_PAGE_COLLECTION` | `e2e_small` | Collection used to check deletion on the last page |
| `LAST_PAGE_TOTAL` | `26` | Its item count (expected to be a 25-item page + 1 item) |
| `IMAGE_COLLECTION` | `e2e_images` | Collection with image Fields (single + array) |
| `COMPOSITE_COLLECTION` | `e2e_composite` | Collection with an image array inside a Composite field |
| `COMPOSITE_ID` | `e2e_gallery_block` | The id of that Composite field definition |
| `SCHEMA_COLLECTION` | `e2e_schema_editor` | Collection assembled from the schema editor screen (deleted at the end of the run) |
| `SCHEMA_BLOCK` | `e2e_block` | The Composite field definition that that array Field holds as its element |

## Language

The harness sets `localStorage['tsubame.language'] = 'en'` in each browser context before opening the
page (`newContext()`). The screen wording comes from the catalog, so without pinning it some checks
**fail depending on the browser's language setting** (in fact, under a Japanese locale the check that
looks for the "閲覧のみ" notice text fails - that is the Japanese wording for "view only"). The switching feature itself is
checked by calling `use()` on top of this stored value. When you change wording, also fix the list
further down in this README and the `has-text` / `aria-label` in `check-ui.mjs`.

The **check names are English**, like the rest of the code and the commit messages
(`CONTRIBUTING.md`): the one exception is the Japanese wording a check *looks for*, which is test
data rather than a label. The lines the suite prints read `PASS  [scenario] What was checked
(measurement)`.

## What Is Checked

- Sign-in from the login form
- The first page has the default 25 items, id ascending, and the pager shows the total count
- The state badge and the Updated column render
- Next page and page-size changes reach the server
- Publishing from the list works (Draft → Published)
- Deleting the last item on the last page goes back to the previous page
- **Saving alone does not publish it** (the delivery API only looks at the Published copy)
- **Publishing is reflected in the delivery API**
- Image library: images can be uploaded, are listed with their names, are served, and can be deleted
- While editing content, an existing image can be selected and saved (the selected id stays on the Item)
- Images can be added to an image array **several at a time**, can be **uploaded in place**, and are saved in that order
- The same works for **an image array inside a Composite field**
- **Schema editing goes through the screen**: create a Collection, add Fields (name, type, width preset),
  add and remove Enum values with chips, **choose a Composite field as the array element type**, save, and read it back through the API
  (the other Collections are created through the API, so this is the only route that exercises this screen in a browser)
- An Enum field defined that way can be **selected and saved on the content edit screen**
- A **Composite array** defined that way can be added element by element, filled in, reordered and saved, and
  reopening it shows the saved order
- That Composite definition can hold **an array of itself** (a block holds a block). The array inside an
  element stops where it is empty, and adding one goes one level deeper
- **Per-Role visibility**: the Editor Role gets no Publish, Delete or account management, and the Viewer Role gets no Save either
- Publishing from the list shows **who published it** on that row
- **Changing your own password** makes the token from before the change return 401, while the session of
  the person who changed it continues (it takes over the new token), and signing in with the new password works
- **Shared preview link**: a Draft that was only saved is visible through a link that opens without a
  token (the delivery API stays on the old Published copy), and rewriting the link's destination returns 401
- **Per-Resource Permissions**: the Accounts screen can save a per-Collection allow, and that Permission
  applies to both the list filtering and the API (a denied Collection does not appear in the list, and hitting it directly also returns 403)
- **Password reset**: a link issued from the Accounts screen can be opened (in a separate browser) to set a new
  password, the sessions from before the reset are cut, and the same link cannot be used twice
- **Brute force is locked out**: the 6th failure returns 429 with `Retry-After`, and during the lock even
  the correct password does not get through, while other accounts are unaffected
- No browser console errors appear during any of this

## Sign-In at the Deployment Target (AWS)

`check-ui.mjs` hits the local password form and `/api/auth/login`. A deployment where Cognito owns sign-in
has **neither** (the endpoint returns 501 and the screen has a button to the hosted page), so
that suite can never reach it. `hosted-signin.mjs` looks at exactly that.

```bash
cd frontend
APP_URL=https://cms.example.com \
ADMIN_USERNAME=cat ADMIN_PASSWORD=... \
PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers node e2e/hosted-signin.mjs
```

What it checks is the PKCE handoff (`login_url` carries `code_challenge` and `redirect_uri`), the return
to the **registered callback**, the code exchange at `/api/auth/callback`, the CMS's own token being
stored, the admin UI appearing, and no console errors along the way. `scripts/smoke-test.sh`, which
checks with `curl`, does not reach this far - a state where the screen renders correctly but only sign-in is
broken is possible.

Prerequisite: the account to sign in with must already exist (an administrator creates it with `AdminCreateUser` +
`AdminSetUserPassword`, and its name is in `BOOTSTRAP_ADMIN_USERNAMES`).

## Account Management at the Deployment Target (AWS)

There is no Cognito emulator, so the SDK calls in `backend/crates/aws/src/provisioner.rs` are **not
executed** locally (only the mapping beyond the seam is visible in unit tests). `hosted-accounts.mjs`
exercises that on a real deployment. Sign-in itself is handled by the shared `hosted-login.mjs` (the two
forms on the hosted page, PKCE, and the "make the user choose a new password" step for the initial temporary password).

```bash
cd frontend
APP_URL=https://cms.example.com \
ADMIN_USERNAME=cat ADMIN_PASSWORD=... \
PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers node e2e/hosted-accounts.mjs
```

1. Sign in as an administrator
2. Create an account with `POST /api/auth/users` (the same call the Accounts screen makes)
3. `POST /api/auth/users/{id}/password-reset` → `kind: "temporary"` and a temporary password
4. **Sign in as that user with that temporary password** → the provider asks for a new password, so decide one and enter it
   this is also the `external_id` verification: if the Cognito `sub` was not recorded at creation time, the user is
   rejected with `not_provisioned`
5. Go back to the administrator and delete
6. After deletion, sign-in fails (the provider refuses)

Even if it fails partway through, no account is left behind (`finally` deletes it).

## Data

Each run recreates `e2e_blog` / `e2e_small` / `e2e_images` / `e2e_composite` and the Composite field definition
`e2e_gallery_block`, plus the throwaway account `e2e-throttle-<time>@example.com` used to check the lock.
The `e2e_schema_editor` used for the schema editing check, and the Composite field definition
`e2e_block` that its array holds as its element (**a block definition that holds an array of itself**), are
created on the spot and deleted at the end (also deleted at the start, in case the previous run died partway)
(deleted if already present). Development data is not touched, and any number of runs gives the
same result. Uploaded images are also deleted at the end of the run.
