// ESLint over the frontend, with the type information TypeScript already has.
//
// `angular-eslint` supplies the parser and the rules that know about Angular (components,
// templates, accessibility); `typescript-eslint`'s checked set is what makes a rule able to say
// "this promise is not awaited" rather than only "this looks like a promise". Prettier owns
// formatting, so its config goes last and switches off every rule that would disagree with it.

import js from '@eslint/js';
import prettier from 'eslint-config-prettier';
import angular from 'angular-eslint';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    // Build output, coverage and the Angular cache are not source.
    ignores: ['dist/', 'coverage/', '.angular/', 'out-tsc/', 'node_modules/'],
  },
  {
    files: ['**/*.js', '**/*.mjs'],
    extends: [js.configs.recommended],
  },
  {
    // The Browser end-to-end script drives a browser through Playwright, so one file holds both
    // sides: Node's own names (`process`) at the top level, and the page's names (`document`,
    // `window`) inside the callbacks that are evaluated there. It is plain JavaScript with no
    // tsconfig, so the type-aware rules below do not apply to it.
    files: ['e2e/**/*.mjs'],
    languageOptions: {
      globals: { ...globals.node, ...globals.browser },
    },
  },
  {
    files: ['**/*.ts'],
    extends: [
      js.configs.recommended,
      ...tseslint.configs.recommendedTypeChecked,
      ...angular.configs.tsRecommended,
    ],
    // A component's `template:` string is a template too, and the template rules apply to it.
    processor: angular.processInlineTemplates,
    languageOptions: {
      parserOptions: {
        // The two tsconfigs (app and spec) are what the lint reads types from.
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      // An unused parameter is sometimes what a signature asks for - a stub that has to match a
      // method it does not use, a callback that only cares about the second argument. The `_`
      // prefix is how this code says "on purpose", so the rule reads it that way.
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
      ],
    },
  },
  {
    files: ['**/*.html'],
    extends: [...angular.configs.templateRecommended, ...angular.configs.templateAccessibility],
  },
  prettier,
);
