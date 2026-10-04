import js from '@eslint/js';
import prettier from 'eslint-config-prettier';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';

export default ts.config(
	js.configs.recommended,
	...ts.configs.recommended,
	...svelte.configs.recommended,
	prettier,
	...svelte.configs.prettier,
	{
		languageOptions: {
			globals: { ...globals.browser, ...globals.node }
		},
		rules: {
			'no-restricted-globals': [
				'error',
				{ name: 'alert', message: 'Use the toast service.' },
				{ name: 'confirm', message: 'Use the confirm service.' },
				{ name: 'prompt', message: 'Use the prompt service.' }
			],
			'@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
			// The app is a single route; links are external and opened through the opener plugin.
			'svelte/no-navigation-without-resolve': 'off',
			// Sets and Maps here are replaced, never mutated in place, when they are state.
			'svelte/prefer-svelte-reactivity': 'off'
		}
	},
	{
		files: ['**/*.svelte', '**/*.svelte.ts'],
		languageOptions: {
			parserOptions: {
				projectService: true,
				extraFileExtensions: ['.svelte'],
				parser: ts.parser
			}
		}
	},
	{
		ignores: [
			'build/',
			'.svelte-kit/',
			'node_modules/',
			'src-tauri/',
			'src/lib/ipc/bindings.ts',
			'src/lib/components/ui/'
		]
	}
);
