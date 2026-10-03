/** Global app settings, loaded once and saved (debounced) on change. */
import { commands, type AppSettings } from '$lib/ipc/bindings';
import { debounce } from '$lib/utils';
import { toast } from './toast.svelte';

const DEFAULTS: AppSettings = {
	theme: 'dark',
	sidebarCollapsed: false,
	favourites: [],
	tabColors: {},
	terminal: {
		fontFamily: 'JetBrains Mono Variable',
		fontSize: 13,
		theme: 'jarvis',
		copyOnSelect: false,
		rightClickPaste: false,
		scrollback: 10000
	},
	downloadDir: null,
	pangolin: { apiUrl: 'https://api.pangolin.net', orgId: '' }
};

class SettingsService {
	value = $state<AppSettings>(structuredClone(DEFAULTS));
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
