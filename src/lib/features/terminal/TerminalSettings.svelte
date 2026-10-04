<!-- Terminal preferences. Changes apply to running terminals immediately. -->
<script lang="ts">
	import Field from '$lib/components/Field.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import type { Bell, CursorStyle, RightClick, TerminalPrefs } from '$lib/ipc';
	import { DEFAULT_SETTINGS, settings } from '$lib/services/settings.svelte';
	import { clamp } from '$lib/utils';
	import { TERMINAL_THEMES, terminalTheme } from './themes';

	const FONTS = [
		'JetBrains Mono Variable',
		'Cascadia Code',
		'Cascadia Mono',
		'Consolas',
		'Fira Code',
		'Source Code Pro',
		'Ubuntu Mono',
		'DejaVu Sans Mono',
		'Menlo',
		'Courier New',
		'monospace'
	];
	const CURSORS: { value: CursorStyle; label: string }[] = [
		{ value: 'block', label: 'Block' },
		{ value: 'bar', label: 'Bar' },
		{ value: 'underline', label: 'Underline' }
	];
	const RIGHT_CLICK: { value: RightClick; label: string }[] = [
		{ value: 'menu', label: 'Open the context menu' },
		{ value: 'paste', label: 'Paste' },
		{ value: 'copyOrPaste', label: 'Copy the selection, otherwise paste' }
	];
	const BELLS: { value: Bell; label: string }[] = [
		{ value: 'none', label: 'Off' },
		{ value: 'visual', label: 'Flash the terminal' },
		{ value: 'sound', label: 'Play a sound' }
	];

	const prefs = $derived(settings.value.terminal);
	const colors = $derived(terminalTheme(prefs.theme));

	function set<K extends keyof TerminalPrefs>(key: K, value: TerminalPrefs[K]): void {
		settings.update((s) => (s.terminal[key] = value));
	}

	/** Read a number input, falling back to the default and clamping to the allowed range. */
	function number(key: 'fontSize' | 'lineHeight' | 'letterSpacing' | 'scrollback', min: number, max: number) {
		return (e: Event & { currentTarget: HTMLInputElement }) => {
			const raw = e.currentTarget.value.trim();
			const parsed = raw === '' ? NaN : Math.round(Number(raw));
			const value = clamp(Number.isNaN(parsed) ? DEFAULT_SETTINGS.terminal[key] : parsed, min, max);
			e.currentTarget.value = String(value);
			set(key, value);
		};
	}

	function reset(): void {
		settings.update((s) => (s.terminal = structuredClone(DEFAULT_SETTINGS.terminal)));
	}
</script>

{#snippet heading(text: string)}
	<h3
		class="col-span-2 mt-2 border-b pb-1 text-xs font-semibold tracking-wide text-muted-foreground uppercase"
	>
		{text}
	</h3>
{/snippet}

{#snippet toggle(label: string, hint: string, key: keyof TerminalPrefs)}
	<label class="col-span-2 flex items-center justify-between gap-4 text-sm">
		<span>
			{label}
			<span class="block text-xs text-muted-foreground">{hint}</span>
		</span>
		<Switch checked={prefs[key] as boolean} onCheckedChange={(v) => set(key, v)} />
	</label>
{/snippet}

<div class="grid grid-cols-2 gap-x-4 gap-y-3 pb-2">
	<div
		class="col-span-2 overflow-hidden rounded-md border px-3 py-2 whitespace-nowrap"
		style="background:{colors.background};color:{colors.foreground};font-family:'{prefs.fontFamily}','JetBrains Mono Variable',monospace;font-size:{prefs.fontSize}px;line-height:{(prefs.lineHeight /
			100) *
			1.2};letter-spacing:{prefs.letterSpacing}px"
		aria-hidden="true"
	>
		<div>
			<span style="color:{colors.green}">admin@server</span>:<span style="color:{colors.blue}">~/app</span>$
			docker ps
		</div>
		<div>
			<span style="color:{colors.yellow}">warning</span>
			<span style="color:{colors.red}">error</span>
			<span style="color:{colors.cyan}">info</span>
			<span style="color:{colors.magenta}">debug</span>
			<span style="background:{colors.selectionBackground}">selected text</span>
		</div>
	</div>

	{@render heading('Appearance')}
	<Field label="Font" hint="A font that is not installed falls back to the default.">
		<SelectField value={prefs.fontFamily} options={FONTS} onchange={(v) => set('fontFamily', v)} />
	</Field>
	<Field label="Colour theme">
		<SelectField
			value={prefs.theme}
			options={Object.entries(TERMINAL_THEMES).map(([value, t]) => ({ value, label: t.label }))}
			onchange={(v) => set('theme', v)}
		/>
	</Field>
	<div class="col-span-2 grid grid-cols-3 gap-4">
		<Field label="Font size (px)">
			<Input type="number" min="8" max="32" value={prefs.fontSize} onchange={number('fontSize', 8, 32)} />
		</Field>
		<Field label="Line height (%)">
			<Input
				type="number"
				min="100"
				max="200"
				step="5"
				value={prefs.lineHeight}
				onchange={number('lineHeight', 100, 200)}
			/>
		</Field>
		<Field label="Letter spacing (px)">
			<Input
				type="number"
				min="-2"
				max="8"
				value={prefs.letterSpacing}
				onchange={number('letterSpacing', -2, 8)}
			/>
		</Field>
	</div>
	<Field label="Cursor">
		<SelectField value={prefs.cursorStyle} options={CURSORS} onchange={(v) => set('cursorStyle', v)} />
	</Field>
	<label class="flex items-center justify-between gap-4 self-end pb-1.5 text-sm">
		Blinking cursor
		<Switch checked={prefs.cursorBlink} onCheckedChange={(v) => set('cursorBlink', v)} />
	</label>

	{@render heading('Mouse and clipboard')}
	{@render toggle('Copy on select', 'Selecting text copies it to the clipboard.', 'copyOnSelect')}
	<Field label="Right-click" hint="Shift+right-click always opens the context menu." class="col-span-2">
		<SelectField value={prefs.rightClick} options={RIGHT_CLICK} onchange={(v) => set('rightClick', v)} />
	</Field>
	{@render toggle(
		'Middle-click pastes',
		'Paste the clipboard with the mouse wheel button.',
		'middleClickPaste'
	)}
	{@render toggle(
		'Ctrl+C / Ctrl+V copy and paste',
		'Ctrl+C copies when text is selected and interrupts otherwise. Ctrl+Shift+C / Ctrl+Shift+V always work.',
		'ctrlCopyPaste'
	)}
	{@render toggle(
		'Confirm multi-line paste',
		'Ask before pasting text with line breaks, which a shell runs line by line.',
		'confirmMultilinePaste'
	)}
	<Field
		label="Word separators"
		hint="Characters that end a word when you double-click to select."
		class="col-span-2"
	>
		<Input
			value={prefs.wordSeparators}
			class="font-mono text-xs"
			spellcheck="false"
			onchange={(e) => set('wordSeparators', e.currentTarget.value)}
		/>
	</Field>

	{@render heading('Behaviour')}
	<Field label="Scrollback lines">
		<Input
			type="number"
			min="500"
			max="100000"
			step="500"
			value={prefs.scrollback}
			onchange={number('scrollback', 500, 100000)}
		/>
	</Field>
	<Field label="Bell">
		<SelectField value={prefs.bell} options={BELLS} onchange={(v) => set('bell', v)} />
	</Field>
	{@render toggle(
		'Scroll to the bottom when typing',
		'Typing leaves the scrollback and returns to the prompt.',
		'scrollOnInput'
	)}

	<div class="col-span-2 mt-2 flex justify-end">
		<Button variant="outline" size="sm" onclick={reset}>Reset terminal settings</Button>
	</div>
</div>
