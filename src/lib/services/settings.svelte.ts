/** Global app settings, loaded once and saved (debounced) on change. */
import { commands, type AppSettings } from '$lib/ipc/bindings';
import { debounce } from '$lib/utils';
import { toast } from './toast.svelte';

export const DEFAULT_SETTINGS: AppSettings = {
	theme: 'dark',
	sidebarCollapsed: false,
	favourites: [],
	tabColors: {},
	terminal: {
		fontFamily: 'JetBrains Mono Variable',
		fontSize: 13,
		lineHeight: 100,
		letterSpacing: 0,
		theme: 'jarvis',
		cursorStyle: 'block',
		cursorBlink: true,
		scrollback: 10000,
		scrollOnInput: true,
		copyOnSelect: false,
		rightClick: 'menu',
		middleClickPaste: false,
		ctrlCopyPaste: false,
		confirmMultilinePaste: false,
		bell: 'none',
		wordSeparators: ' ()[]{}\',"`'
	},
	downloadDir: null,
	pangolin: { apiUrl: 'https://api.pangolin.net', basePath: '', orgId: '' }
};

class SettingsService {
	value = $state<AppSettings>(structuredClone(DEFAULT_SETTINGS));
	loaded = $state(false);

	#save = debounce(() => {
		commands.settingsSet($state.snapshot(this.value)).catch((e) => toast.error(e, 'Could not save settings'));
	}, 300);

	async load(): Promise<void> {
		this.value = await commands.settingsGet();
		this.loaded = true;
		this.applyTheme();
	}

	/** Mutate settings in place, then persist. */
	update(edit: (settings: AppSettings) => void): void {
		edit(this.value);
		this.applyTheme();
		this.#save();
	}

	get isDark(): boolean {
		const theme = this.value.theme;
		if (theme === 'system') return window.matchMedia('(prefers-color-scheme: dark)').matches;
		return theme === 'dark';
	}

	applyTheme(): void {
		document.documentElement.classList.toggle('dark', this.isDark);
	}
}

export const settings = new SettingsService();
