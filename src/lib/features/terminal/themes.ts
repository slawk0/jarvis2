import type { ITheme } from '@xterm/xterm';

export interface TerminalTheme {
	label: string;
	theme: ITheme;
}

export const TERMINAL_THEMES: Record<string, TerminalTheme> = {
	jarvis: {
		label: 'Jarvis Dark',
		theme: {
			background: '#0b0f14',
			foreground: '#d7dde5',
			cursor: '#2dd4bf',
			cursorAccent: '#0b0f14',
			selectionBackground: '#2dd4bf44',
			black: '#1b222c',
			red: '#f47067',
			green: '#57d18a',
			yellow: '#e3b341',
			blue: '#6cb6ff',
			magenta: '#c98fff',
			cyan: '#39c5cf',
			white: '#c9d1d9',
			brightBlack: '#6e7b8b',
			brightRed: '#ff938a',
			brightGreen: '#7ee2a8',
			brightYellow: '#f2cc60',
			brightBlue: '#96d0ff',
			brightMagenta: '#dcbdfb',
			brightCyan: '#56d4dd',
			brightWhite: '#f0f6fc'
		}
	},
	light: {
		label: 'Light',
		theme: {
			background: '#fbfcfd',
			foreground: '#24292f',
			cursor: '#0f766e',
			cursorAccent: '#fbfcfd',
			selectionBackground: '#0f766e33',
			black: '#24292f',
			red: '#cf222e',
			green: '#116329',
			yellow: '#7d4e00',
			blue: '#0969da',
			magenta: '#8250df',
			cyan: '#1b7c83',
			white: '#6e7781',
			brightBlack: '#57606a',
			brightRed: '#a40e26',
			brightGreen: '#1a7f37',
			brightYellow: '#633c01',
			brightBlue: '#218bff',
			brightMagenta: '#a475f9',
			brightCyan: '#3192aa',
			brightWhite: '#8c959f'
		}
	},
	solarized: {
		label: 'Solarized Dark',
		theme: {
			background: '#002b36',
			foreground: '#93a1a1',
			cursor: '#93a1a1',
			cursorAccent: '#002b36',
			selectionBackground: '#586e7566',
			black: '#073642',
			red: '#dc322f',
			green: '#859900',
			yellow: '#b58900',
			blue: '#268bd2',
			magenta: '#d33682',
			cyan: '#2aa198',
			white: '#eee8d5',
			brightBlack: '#586e75',
			brightRed: '#cb4b16',
			brightGreen: '#586e75',
			brightYellow: '#657b83',
			brightBlue: '#839496',
			brightMagenta: '#6c71c4',
			brightCyan: '#93a1a1',
			brightWhite: '#fdf6e3'
		}
	},
	dracula: {
		label: 'Dracula',
		theme: {
			background: '#282a36',
			foreground: '#f8f8f2',
			cursor: '#f8f8f2',
			cursorAccent: '#282a36',
			selectionBackground: '#44475a',
			black: '#21222c',
			red: '#ff5555',
			green: '#50fa7b',
			yellow: '#f1fa8c',
			blue: '#bd93f9',
			magenta: '#ff79c6',
			cyan: '#8be9fd',
			white: '#f8f8f2',
			brightBlack: '#6272a4',
			brightRed: '#ff6e6e',
			brightGreen: '#69ff94',
			brightYellow: '#ffffa5',
			brightBlue: '#d6acff',
			brightMagenta: '#ff92df',
			brightCyan: '#a4ffff',
			brightWhite: '#ffffff'
		}
	}
};

export function terminalTheme(name: string): ITheme {
	return (TERMINAL_THEMES[name] ?? TERMINAL_THEMES.jarvis).theme;
}
