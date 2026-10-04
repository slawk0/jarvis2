<!-- One PTY session rendered with xterm.js. -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { FitAddon } from '@xterm/addon-fit';
	import { WebLinksAddon } from '@xterm/addon-web-links';
	import { Terminal } from '@xterm/xterm';
	import '@xterm/xterm/css/xterm.css';
	import { Button } from '$lib/components/ui/button';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { api, events, toIpcError, type TerminalPrefs, type TerminalTarget } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { settings } from '$lib/services/settings.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { debounce } from '$lib/utils';
	import { terminalTheme } from './themes';

	interface Props {
		target: TerminalTarget;
		/** Shown and sized; hidden sessions keep running. */
		active: boolean;
		onstatus?: (running: boolean) => void;
		onedit?: (path: string) => void;
	}

	let { target, active, onstatus, onedit }: Props = $props();

	const PASTE_CHUNK = 16 * 1024;

	let host = $state<HTMLDivElement | null>(null);
	let ended = $state<string | null>(null);
	let connecting = $state(true);
	let flash = $state(false);
	let term: Terminal | null = null;
	let fit: FitAddon | null = null;
	let id: string | null = null;
	let early: { id: string; data: string }[] = [];
	let writes: Promise<unknown> = Promise.resolve();

	/** Writes are chained so chunks of a large paste arrive in order. */
	function send(data: string): void {
		const session = id;
		if (!session || ended) return;
		for (let i = 0; i < data.length; i += PASTE_CHUNK) {
			const chunk = data.slice(i, i + PASTE_CHUNK);
			writes = writes.then(() => api.terminalWrite(session, chunk)).catch(() => {});
		}
	}

	async function start(): Promise<void> {
		if (!term) return;
		ended = null;
		connecting = true;
		id = null;
		early = [];
		try {
			fit?.fit();
			const session = await api.terminalOpen($state.snapshot(target), term.cols, term.rows);
			id = session;
			for (const item of early) if (item.id === session) term.write(item.data);
			early = [];
			onstatus?.(true);
			term.focus();
		} catch (raw) {
			const error = toIpcError(raw);
			ended = error.code === 'CANCELLED' ? 'Cancelled.' : error.message;
		} finally {
			connecting = false;
		}
	}

	export function restart(): void {
		if (id) void api.terminalClose(id);
		id = null;
		term?.reset();
		void start();
	}

	export function run(command: string): void {
		send(`${command}\r`);
		term?.focus();
	}

	export function sessionId(): string | null {
		return ended ? null : id;
	}

	export function focus(): void {
		term?.focus();
	}

	async function copySelection(): Promise<void> {
		const text = term?.getSelection();
		if (text) await writeText(text);
	}

	async function paste(): Promise<void> {
		let text: string;
		try {
			text = await readText();
		} catch (error) {
			toast.error(String(error), 'Could not read the clipboard');
			return;
		}
		if (!text) return;
		const lines = text.trimEnd().split(/\r?\n/);
		if (settings.value.terminal.confirmMultilinePaste && lines.length > 1) {
			const ok = await confirm({
				title: `Paste ${lines.length} lines?`,
				message: 'A shell runs pasted text line by line.',
				detail: lines.slice(0, 20).join('\n') + (lines.length > 20 ? '\n…' : ''),
				confirmLabel: 'Paste'
			});
			if (!ok) return;
		}
		term?.paste(text);
		term?.focus();
	}

	function bell(): void {
		const mode = settings.value.terminal.bell;
		if (mode === 'visual') {
			flash = true;
			setTimeout(() => (flash = false), 150);
		} else if (mode === 'sound') {
			const audio = new AudioContext();
			const tone = audio.createOscillator();
			const gain = audio.createGain();
			tone.frequency.value = 800;
			gain.gain.value = 0.08;
			tone.connect(gain).connect(audio.destination);
			tone.start();
			tone.stop(audio.currentTime + 0.12);
			tone.onended = () => void audio.close();
		}
	}

	/** The xterm options that come from the terminal preferences. */
	function options(prefs: TerminalPrefs) {
		return {
			fontFamily: `'${prefs.fontFamily}', 'JetBrains Mono Variable', monospace`,
			fontSize: prefs.fontSize,
			lineHeight: prefs.lineHeight / 100,
			letterSpacing: prefs.letterSpacing,
			theme: terminalTheme(prefs.theme),
			cursorStyle: prefs.cursorStyle,
			cursorBlink: prefs.cursorBlink,
			scrollback: prefs.scrollback,
			scrollOnUserInput: prefs.scrollOnInput,
			wordSeparator: prefs.wordSeparators
		};
	}

	const resize = debounce(() => {
		if (!term || !fit || !host || host.clientWidth === 0) return;
		fit.fit();
		if (id && !ended) void api.terminalResize(id, term.cols, term.rows).catch(() => {});
	}, 60);

	onMount(() => {
		term = new Terminal({
			...options(settings.value.terminal),
			allowProposedApi: true,
			macOptionIsMeta: true
		});
		fit = new FitAddon();
		term.loadAddon(fit);
		term.loadAddon(new WebLinksAddon((_event, uri) => void openUrl(uri)));
		term.open(host!);

		term.onData(send);
		term.onSelectionChange(() => {
			if (settings.value.terminal.copyOnSelect) void copySelection();
		});
		term.onBell(bell);
		term.attachCustomKeyEventHandler((e) => {
			if (e.type !== 'keydown' || !e.ctrlKey || e.altKey) return true;
			if (!e.shiftKey) {
				if (!settings.value.terminal.ctrlCopyPaste) return true;
				// Without a selection Ctrl+C stays the interrupt key.
				if (e.code === 'KeyC' && !term?.hasSelection()) return true;
			}
			if (e.code === 'KeyC') {
				e.preventDefault();
				void copySelection();
				term?.clearSelection();
				return false;
			}
			if (e.code === 'KeyV') {
				e.preventDefault();
				void paste();
				return false;
			}
			return true;
		});

		const offData = events.terminalData.listen(({ payload }) => {
			if (id === null) early.push(payload);
			else if (payload.id === id) term?.write(payload.data);
		});
		const offExit = events.terminalExit.listen(({ payload }) => {
			if (payload.id !== id) return;
			ended = payload.error
				? toIpcError(payload.error).message
				: `Session ended${payload.exitCode ? ` (exit code ${payload.exitCode})` : ''}.`;
			onstatus?.(false);
		});

		const observer = new ResizeObserver(resize);
		observer.observe(host!);
		void start();

		return () => {
			observer.disconnect();
			resize.cancel();
			void offData.then((off) => off());
			void offExit.then((off) => off());
			if (id) void api.terminalClose(id);
			onstatus?.(false);
			term?.dispose();
			term = null;
		};
	});

	// Apply appearance changes from Settings to running terminals.
	$effect(() => {
		const next = options(settings.value.terminal);
		if (!term) return;
		Object.assign(term.options, next);
		resize();
	});

	$effect(() => {
		if (active) {
			resize();
			term?.focus();
		}
	});

	function onContextMenu(e: MouseEvent): void {
		// Shift+right-click always opens the menu.
		const mode = settings.value.terminal.rightClick;
		if (mode === 'menu' || e.shiftKey) return;
		e.preventDefault();
		e.stopPropagation();
		if (mode === 'copyOrPaste' && term?.hasSelection()) {
			void copySelection();
			term.clearSelection();
		} else {
			void paste();
		}
	}

	function onMouseDown(e: MouseEvent): void {
		// A program that tracks the mouse (vim, htop) gets the middle button itself.
		if (e.button !== 1 || !settings.value.terminal.middleClickPaste) return;
		if (term?.modes.mouseTrackingMode !== 'none') return;
		e.preventDefault();
		e.stopPropagation();
		void paste();
	}

	function selectedPath(): string | null {
		const text = term?.getSelection().trim() ?? '';
		return text.startsWith('/') && !text.includes('\n') ? text : null;
	}
</script>

<div class="relative h-full w-full" class:hidden={!active}>
	<ContextMenu.Root>
		<ContextMenu.Trigger>
			{#snippet child({ props })}
				<div
					{...props}
					bind:this={host}
					class="absolute inset-0 px-2 pt-1.5"
					style="background: {terminalTheme(settings.value.terminal.theme).background}"
					oncontextmenucapture={onContextMenu}
					onmousedowncapture={onMouseDown}
				></div>
			{/snippet}
		</ContextMenu.Trigger>
		<ContextMenu.Content class="w-56">
			<ContextMenu.Item onclick={copySelection}
				>Copy<ContextMenu.Shortcut>Ctrl+Shift+C</ContextMenu.Shortcut></ContextMenu.Item
			>
			<ContextMenu.Item onclick={paste}
				>Paste<ContextMenu.Shortcut>Ctrl+Shift+V</ContextMenu.Shortcut></ContextMenu.Item
			>
			<ContextMenu.Item onclick={() => term?.selectAll()}>Select all</ContextMenu.Item>
			<ContextMenu.Item onclick={() => term?.clear()}>Clear</ContextMenu.Item>
			<ContextMenu.Separator />
			<ContextMenu.Item
				onclick={() => {
					const path = selectedPath();
					if (path) onedit?.(path);
					else toast.info('Select an absolute path in the terminal first');
				}}
			>
				Open selected path in editor
			</ContextMenu.Item>
		</ContextMenu.Content>
	</ContextMenu.Root>

	{#if connecting}
		<p class="absolute top-2 left-3 text-xs text-muted-foreground">Connecting…</p>
	{/if}
	{#if flash}
		<div class="pointer-events-none absolute inset-0 bg-foreground/15"></div>
	{/if}
	{#if ended}
		<div
			class="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t bg-popover px-3 py-2 text-sm"
			role="status"
		>
			<span class="selectable min-w-0 flex-1 truncate">{ended}</span>
			<Button size="sm" onclick={restart}>Restart</Button>
		</div>
	{/if}
</div>
