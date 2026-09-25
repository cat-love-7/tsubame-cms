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
    /*
     * One worker per core is what vitest picks on its own, and each worker is a whole
     * browser-less Angular environment: about 250 MiB resident, times the cores - 8.5 GiB on a
     * 32-core machine, for a suite that finishes in six seconds either way. Four workers (the
     * cores a CI runner has) measured about 2.7 GiB at the same wall clock, so the pool is
     * capped rather than left to the machine.
     */
    maxWorkers: 4,
  },
});
