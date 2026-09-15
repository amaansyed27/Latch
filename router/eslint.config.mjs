import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: ['dist/**', 'public/**', 'web/test-results/**', 'web/playwright-report/**'],
  },
  {
    ...js.configs.recommended,
    languageOptions: {
      globals: globals.node,
    },
  },
  ...tseslint.configs.recommended,
  { files: ['web/**/*.js'], languageOptions: { globals: globals.browser } },
  {
    files: ['**/*.ts', '**/*.tsx'],
    languageOptions: {
      globals: { ...globals.node, ...globals.browser },
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'error',
    },
  },
  {
    files: ['src/mcp-server.ts'],
    rules: {
      'no-shadow-restricted-names': 'off',
    },
  },
);
