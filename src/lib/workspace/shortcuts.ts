/**
 * App-wide keyboard shortcuts. Registered on `window` in the capture phase so
 * they work even while a terminal or editor has focus.
 */
import { settings } from '$lib/services/settings.svelte';
import { workspace } from './workspace.svelte';

export interface Shortcut {
	keys: string;
	description: string;
}

/** Listed in the shortcuts help dialog. The last two are handled by their components. */
export const SHORTCUTS: Shortcut[] = [
	{ keys: 'Ctrl+N', description: 'Split the focused pane' },
	{ keys: 'Ctrl+W', description: 'Close the focused pane' },
	{ keys: 'Ctrl+1 … Ctrl+4', description: 'Focus pane 1–4 (reading order)' },
	{ keys: 'Ctrl+Tab', description: 'Next tab' },
	{ keys: 'Ctrl+Shift+Tab', description: 'Previous tab' },
	{ keys: 'Ctrl+Shift+T', description: 'Open Terminal' },
	{ keys: 'Ctrl+Alt+B', description: 'Toggle sidebar' },
	{ keys: 'Ctrl+Shift+H', description: 'Show keyboard shortcuts' },
	{ keys: 'Ctrl+Shift+C', description: 'Copy in terminal' },
	{ keys: 'Ctrl+Shift+V', description: 'Paste in terminal' },
	{ keys: 'Ctrl+S', description: 'Save in editors' },
	{ keys: 'Mouse back button', description: 'Go back' },
	{ keys: 'Esc', description: 'Close the topmost dialog or menu' }
];

/** Returns true when the event was a workspace shortcut and has been handled. */
export function handleShortcut(e: KeyboardEvent): boolean {
	if (!e.ctrlKey && !e.metaKey) return false;
	const key = e.key.toLowerCase();
	const plain = !e.shiftKey && !e.altKey;
	const shift = e.shiftKey && !e.altKey;
	const alt = e.altKey && !e.shiftKey;

	if (plain && key === 'n') {
		workspace.split('vertical');
	} else if (plain && key === 'w') {
		void workspace.close();
	} else if (plain && ['1', '2', '3', '4'].includes(key)) {
		workspace.focusByIndex(Number(key) - 1);
	} else if (key === 'tab' && !e.altKey) {
		workspace.cycleTab(e.shiftKey ? -1 : 1);
	} else if (shift && key === 't') {
		workspace.openTab('terminal');
	} else if (alt && key === 'b') {
		settings.update((s) => (s.sidebarCollapsed = !s.sidebarCollapsed));
	} else if (shift && key === 'h') {
		workspace.shortcutsOpen = !workspace.shortcutsOpen;
	} else {
		return false;
	}
	e.preventDefault();
	e.stopPropagation();
	return true;
}
