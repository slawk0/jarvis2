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
	},
	oneDark: {
		label: 'One Dark',
		theme: {
			background: '#282c34',
			foreground: '#abb2bf',
			cursor: '#528bff',
			cursorAccent: '#282c34',
			selectionBackground: '#3e4451',
			black: '#3f4451',
			red: '#e06c75',
			green: '#98c379',
			yellow: '#d19a66',
			blue: '#61afef',
			magenta: '#c678dd',
			cyan: '#56b6c2',
			white: '#d7dae0',
			brightBlack: '#4f5666',
			brightRed: '#ff7b86',
			brightGreen: '#b1e18b',
			brightYellow: '#efb074',
			brightBlue: '#67cdff',
			brightMagenta: '#e48bff',
			brightCyan: '#63d4e0',
			brightWhite: '#ffffff'
		}
	},
	nord: {
		label: 'Nord',
		theme: {
			background: '#2e3440',
			foreground: '#d8dee9',
			cursor: '#d8dee9',
			cursorAccent: '#2e3440',
			selectionBackground: '#434c5e',
			black: '#3b4252',
			red: '#bf616a',
			green: '#a3be8c',
			yellow: '#ebcb8b',
			blue: '#81a1c1',
			magenta: '#b48ead',
			cyan: '#88c0d0',
			white: '#e5e9f0',
			brightBlack: '#4c566a',
			brightRed: '#bf616a',
			brightGreen: '#a3be8c',
			brightYellow: '#ebcb8b',
			brightBlue: '#81a1c1',
			brightMagenta: '#b48ead',
			brightCyan: '#8fbcbb',
			brightWhite: '#eceff4'
		}
	},
	gruvbox: {
		label: 'Gruvbox Dark',
		theme: {
			background: '#282828',
			foreground: '#ebdbb2',
			cursor: '#ebdbb2',
			cursorAccent: '#282828',
			selectionBackground: '#504945',
			black: '#282828',
			red: '#cc241d',
			green: '#98971a',
			yellow: '#d79921',
			blue: '#458588',
			magenta: '#b16286',
			cyan: '#689d6a',
			white: '#a89984',
			brightBlack: '#928374',
			brightRed: '#fb4934',
			brightGreen: '#b8bb26',
			brightYellow: '#fabd2f',
			brightBlue: '#83a598',
			brightMagenta: '#d3869b',
			brightCyan: '#8ec07c',
			brightWhite: '#ebdbb2'
		}
	},
	tokyoNight: {
		label: 'Tokyo Night',
		theme: {
			background: '#1a1b26',
			foreground: '#c0caf5',
			cursor: '#c0caf5',
			cursorAccent: '#1a1b26',
			selectionBackground: '#33467c',
			black: '#15161e',
			red: '#f7768e',
			green: '#9ece6a',
			yellow: '#e0af68',
			blue: '#7aa2f7',
			magenta: '#bb9af7',
			cyan: '#7dcfff',
			white: '#a9b1d6',
			brightBlack: '#414868',
			brightRed: '#f7768e',
			brightGreen: '#9ece6a',
			brightYellow: '#e0af68',
			brightBlue: '#7aa2f7',
			brightMagenta: '#bb9af7',
			brightCyan: '#7dcfff',
			brightWhite: '#c0caf5'
		}
	},
	black: {
		label: 'Classic (black)',
		theme: {
			background: '#000000',
			foreground: '#bbbbbb',
			cursor: '#00ff00',
			cursorAccent: '#000000',
			selectionBackground: '#555555',
			black: '#000000',
			red: '#bb0000',
			green: '#00bb00',
			yellow: '#bbbb00',
			blue: '#5555ff',
			magenta: '#bb00bb',
			cyan: '#00bbbb',
			white: '#bbbbbb',
			brightBlack: '#555555',
			brightRed: '#ff5555',
			brightGreen: '#55ff55',
			brightYellow: '#ffff55',
			brightBlue: '#7777ff',
			brightMagenta: '#ff55ff',
			brightCyan: '#55ffff',
			brightWhite: '#ffffff'
		}
	}
};

export function terminalTheme(name: string): ITheme {
	return (TERMINAL_THEMES[name] ?? TERMINAL_THEMES.jarvis).theme;
}
