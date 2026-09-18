# SlCms

The CMS's admin interface: an Angular 22 standalone application (zoneless, signals, OnPush) that
talks to the API in `sl_cms/`. It is served by the same binary in a deployment, and by the dev
server below while working on it.

The rules this code follows - route parameters, the `apiUrl` boundary, the translation-key
contract, the `error-codes.json` contract, how `repositories/` and `services/` divide the work -
are in [`doc/frontend-design.md`](../doc/frontend-design.md). The repository root has an
`.github/copilot-instructions.md` for the same conventions in short form.

## Development server

To start a local development server, run:

```bash
ng serve
```

Once the server is running, open your browser and navigate to `http://localhost:4200/`. The application will automatically reload whenever you modify any of the source files.

## Code scaffolding

Angular CLI includes powerful code scaffolding tools. To generate a new component, run:

```bash
ng generate component component-name
```

For a complete list of available schematics (such as `components`, `directives`, or `pipes`), run:

```bash
ng generate --help
```

## Building

To build the project run:

```bash
ng build
```

This will compile your project and store the build artifacts in the `dist/` directory. By default, the production build optimizes your application for performance and speed.

## Running unit tests

To execute unit tests with the [Vitest](https://vitest.dev/) test runner, use the following command:

```bash
ng test
```

For coverage (needs `@vitest/coverage-v8`, which is a dev dependency):

```bash
npm run test:coverage
```

The coverage run passes `--no-isolate`. Every spec file is otherwise given its own jsdom and
Angular environment, and under instrumentation that contention makes a synchronous test
occasionally overshoot Vitest's 5s timeout and be reported as a failure. One shared environment
is faster and stable; `ng test` keeps the isolated default. The end-to-end check
(`e2e/README.md`) is not part of either number.

## Running end-to-end tests

The e2e check drives a real browser against a running backend (`npm run e2e`, which is
`node e2e/check-ui.mjs`). It seeds what it needs through the API, so it can be run against a
fresh database; `scripts/test-e2e.sh` starts both halves and is what CI runs. See
[`e2e/README.md`](e2e/README.md) for what it covers and how to point it somewhere else.

```bash
npm run e2e
```

## Linting and formatting

Both are checks in CI, and both are one command:

```bash
npm run lint          # ESLint (eslint.config.mjs)
npm run format:check  # Prettier; `npm run format` rewrites
```

The lint reads the two tsconfigs, so a promise nobody awaits, an `any` that leaks out of a
library's types or an unused import is an error rather than a note. Formatting stays Prettier's
job: the ESLint config turns off every rule that would disagree with it.

## Additional Resources

For the Angular CLI itself, see the
[CLI Overview and Command Reference](https://angular.dev/tools/cli).
