/**
 * Monaco setup with locally bundled workers (the app must work offline and
 * the CSP forbids loading anything from a CDN). Imported lazily by the editor
 * components so it stays out of the initial bundle.
 */
import * as monaco from 'monaco-editor';
import editorWorker from 'monaco-editor/editor/editor.worker?worker';
import cssWorker from 'monaco-editor/language/css/css.worker?worker';
import htmlWorker from 'monaco-editor/language/html/html.worker?worker';
import jsonWorker from 'monaco-editor/language/json/json.worker?worker';
import tsWorker from 'monaco-editor/language/typescript/ts.worker?worker';
import { parse as parseToml } from 'smol-toml';
import { parseDocument } from 'yaml';

self.MonacoEnvironment = {
	getWorker(_workerId: string, label: string) {
		switch (label) {
			case 'json':
				return new jsonWorker();
			case 'css':
			case 'scss':
			case 'less':
				return new cssWorker();
			case 'html':
			case 'handlebars':
			case 'razor':
				return new htmlWorker();
			case 'typescript':
			case 'javascript':
				return new tsWorker();
			default:
				return new editorWorker();
		}
	}
};

monaco.editor.defineTheme('jarvis-dark', {
	base: 'vs-dark',
	inherit: true,
	rules: [],
	colors: {
		'editor.background': '#0b0f14',
		'editor.lineHighlightBackground': '#141b24',
		'editorLineNumber.foreground': '#4b5766',
		'editorCursor.foreground': '#2dd4bf',
		'editor.selectionBackground': '#2dd4bf33',
		'editorWidget.background': '#141b24',
		'editorGutter.background': '#0b0f14'
	}
});
monaco.editor.defineTheme('jarvis-light', {
	base: 'vs',
	inherit: true,
	rules: [],
	colors: { 'editorCursor.foreground': '#0f766e' }
});

const BY_EXTENSION: Record<string, string> = {
	json: 'json',
	yml: 'yaml',
	yaml: 'yaml',
	toml: 'toml',
	xml: 'xml',
	html: 'html',
	htm: 'html',
	css: 'css',
	scss: 'scss',
	js: 'javascript',
	mjs: 'javascript',
	cjs: 'javascript',
	ts: 'typescript',
	py: 'python',
	rb: 'ruby',
	php: 'php',
	go: 'go',
	rs: 'rust',
	sh: 'shell',
	bash: 'shell',
	zsh: 'shell',
	sql: 'sql',
	md: 'markdown',
	ini: 'ini',
	conf: 'ini',
	cnf: 'ini',
	cfg: 'ini',
	env: 'ini',
	service: 'ini',
	timer: 'ini',
	socket: 'ini',
	lua: 'lua',
	pl: 'perl',
	java: 'java',
	c: 'c',
	h: 'c',
	cpp: 'cpp',
	dockerfile: 'dockerfile'
};

/** Editor language for a file path. */
export function languageFor(path: string): string {
	const name = path.split('/').pop()?.toLowerCase() ?? '';
	if (name === 'dockerfile' || name.startsWith('dockerfile.')) return 'dockerfile';
	if (name === 'makefile') return 'makefile';
	if (name.startsWith('.env')) return 'ini';
	if (name.startsWith('.bash') || name === '.profile' || name === '.zshrc') return 'shell';
	if (path.includes('/nginx/') || path.includes('/sites-') || name === 'nginx.conf') return 'ini';
	const ext = name.includes('.') ? name.split('.').pop()! : '';
	return BY_EXTENSION[ext] ?? 'plaintext';
}

export interface SyntaxProblem {
	message: string;
	line: number;
	column: number;
}

/**
 * Syntax check for formats Monaco has no validating worker for.
 * JSON is validated by Monaco itself.
 */
export function checkSyntax(language: string, text: string): SyntaxProblem[] {
	try {
		if (language === 'yaml') {
			const doc = parseDocument(text, { prettyErrors: true });
			return doc.errors.map((e) => ({
				message: e.message.split('\n')[0],
				line: e.linePos?.[0].line ?? 1,
				column: e.linePos?.[0].col ?? 1
			}));
		}
		if (language === 'toml') {
			parseToml(text);
		} else if (language === 'xml') {
			const doc = new DOMParser().parseFromString(text, 'application/xml');
			const error = doc.querySelector('parsererror');
			if (error) {
				const match = /line (\d+)[^\d]+column (\d+)/i.exec(error.textContent ?? '');
				return [
					{
						message: (error.textContent ?? 'Invalid XML').split('\n')[0].trim(),
						line: match ? Number(match[1]) : 1,
						column: match ? Number(match[2]) : 1
					}
				];
			}
		} else if (language === 'ini') {
			const problems: SyntaxProblem[] = [];
			text.split('\n').forEach((raw, index) => {
				const line = raw.trim();
				if (line.startsWith('[') && !/^\[[^\]]*\]\s*([#;].*)?$/.test(line)) {
					problems.push({ message: 'Unclosed section header', line: index + 1, column: 1 });
				}
			});
			return problems;
		}
	} catch (error) {
		const e = error as { message?: string; line?: number; column?: number };
		return [
			{ message: (e.message ?? 'Syntax error').split('\n')[0], line: e.line ?? 1, column: e.column ?? 1 }
		];
	}
	return [];
}

export function applyProblems(model: monaco.editor.ITextModel, problems: SyntaxProblem[]): void {
	monaco.editor.setModelMarkers(
		model,
		'jarvis-syntax',
		problems.map((p) => ({
			severity: monaco.MarkerSeverity.Error,
			message: p.message,
			startLineNumber: p.line,
			startColumn: p.column,
			endLineNumber: p.line,
			endColumn: model.getLineMaxColumn(Math.min(p.line, model.getLineCount()))
		}))
	);
}

export { monaco };
