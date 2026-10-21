// ESLint for the app and the harness/Playwright configs, which nothing else
// type-checks (`ng build` compiles only what `src/main.ts` imports). Type-aware
// typescript-eslint plus the Angular rules (no inline templates or styles,
// template a11y).

import angular from 'angular-eslint';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  // An eslint-disable that disables nothing is an error, not a warning.
  { linterOptions: { reportUnusedDisableDirectives: 'error' } },
  {
    files: ['src/**/*.ts', 'e2e/**/*.ts', '*.config.ts'],
    extends: [
      ...tseslint.configs.recommendedTypeChecked,
      ...tseslint.configs.stylisticTypeChecked,
      ...angular.configs.tsRecommended,
    ],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    processor: angular.processInlineTemplates,
    rules: {
      '@angular-eslint/component-max-inline-declarations': ['error', { template: 0, styles: 0 }],
      // `x as Shape` is a claim, not a check, and the one way past
      // DL-ANGULAR-STRINGIFIED-OBJECT. Narrow at the boundary.
      '@typescript-eslint/no-unsafe-type-assertion': 'error',
      '@typescript-eslint/no-empty-function': 'off',
    },
  },
  {
    // Test doubles may assert their interface; a wrong one fails a test, never a user.
    files: ['src/**/*.spec.ts'],
    rules: {
      '@typescript-eslint/no-unsafe-type-assertion': 'off',
    },
  },
  {
    files: ['src/**/*.html'],
    extends: [...angular.configs.templateRecommended, ...angular.configs.templateAccessibility],
  },
);
