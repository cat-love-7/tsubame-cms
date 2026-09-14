import { defineConfig } from 'vitest/config';

/**
 * Vitest options for `ng test`, loaded through `angular.json`'s `runnerConfig`.
 *
 * The suite is CPU bound - jsdom and an Angular environment per spec file, plus template
 * compilation - and under load a test that only ever runs synchronously can be reported as a
 * timeout because its worker was starved, not because anything hung. The tests themselves are
 * sub-second, so the ceiling is raised rather than the suite being made to look flaky; a real
 * hang still fails, just later.
 */
export default defineConfig({
  test: {
    testTimeout: 20_000,
  },
});
