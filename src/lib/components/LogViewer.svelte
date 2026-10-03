<!--
	The shared log / command-output viewer: filter box, follow until the user
	scrolls up, pause, copy, download, optional severity colouring. Accepts a
	static string or a live Job. Lines are windowed, so large logs stay fast.
-->
<script lang="ts" module>
	// eslint-disable-next-line no-control-regex
	const ANSI = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*(\x07|\x1b\\)|\x1b[@-_]/g;

	/** Strip escape sequences and resolve carriage-return overwrites. */
	export function cleanLine(line: string): string {
		const text = line.replace(ANSI, '');
		const cr = text.lastIndexOf('\r', text.length - 2);
		return (cr >= 0 ? text.slice(cr + 1) : text).replace(/\r$/, '');
	}

	export type Severity = 'error' | 'warn' | 'debug' | null;

	export function severityOf(line: string): Severity {
		if (/\b(error|err|fatal|crit(ical)?|emerg|alert|panic|fail(ed|ure)?|denied)\b/i.test(line)) return 'error';
		if (/\bwarn(ing)?\b/i.test(line)) return 'warn';
		if (/\b(debug|trace)\b/i.test(line)) return 'debug';
		return null;
	}
</script>

<script lang="ts">
	import type { Snippet } from 'svelte';
	import { save } from '@tauri-apps/plugin-dialog';
	import ArrowDownToLine from '@lucide/svelte/icons/arrow-down-to-line';
	import Copy from '@lucide/svelte/icons/copy';
	import Download from '@lucide/svelte/icons/download';
	import Eraser from '@lucide/svelte/icons/eraser';
	import Pause from '@lucide/svelte/icons/pause';
	import Play from '@lucide/svelte/icons/play';
	import { api } from '$lib/ipc';
	import type { Job } from '$lib/services/jobs.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { cn, copyText } from '$lib/utils';
	import IconButton from './IconButton.svelte';
	import SearchInput from './SearchInput.svelte';

	interface Props {
		/** Static text, or a job whose output is followed live. */
		source: string | Job | null;
		maxLines?: number;
		severity?: boolean;
		/** Show the toolbar (filter, pause, copy…). */
		controls?: boolean;
		/** Allow pausing and clearing (meaningful for live streams). */
		live?: boolean;
		downloadName?: string;
		placeholder?: string;
		/** Extra toolbar content on the left. */
		toolbar?: Snippet;
		class?: string;
	}

	let {
		source,
		maxLines = 20_000,
		severity = false,
		controls = true,
		live = false,
		downloadName = 'output.log',
		placeholder = 'No output yet',
		toolbar,
		class: className
	}: Props = $props();

	const LINE_HEIGHT = 18;
	const OVERSCAN = 30;

	// Line storage is deliberately not reactive; `version` signals changes.
	let lines: string[] = [];
	let partial = '';
	let version = $state(0);
	let paused = $state(false);
	let pending = '';
	let filter = $state('');
	let following = $state(true);
	let scroller = $state<HTMLDivElement | null>(null);
	let scrollTop = $state(0);
	let viewport = $state(400);
	let frame = 0;

	function ingest(chunk: string): void {
		const parts = (partial + chunk).split('\n');
		partial = parts.pop() ?? '';
		for (const part of parts) lines.push(cleanLine(part));
		if (lines.length > maxLines) lines = lines.slice(lines.length - maxLines);
		if (!frame) {
			frame = requestAnimationFrame(() => {
				frame = 0;
				version++;
			});
		}
	}

	function reset(text = ''): void {
		lines = [];
		partial = '';
		pending = '';
		if (text) ingest(text);
		version++;
	}

	$effect(() => {
		const current = source;
		if (current === null || typeof current === 'string') {
			reset(current ?? '');
			return;
		}
		reset(current.log);
		return current.onOutput((chunk) => {
			if (paused) pending += chunk;
			else ingest(chunk);
		});
	});

	function togglePause(): void {
		paused = !paused;
		if (!paused && pending) {
			ingest(pending);
			pending = '';
		}
	}

	const visibleLines = $derived.by(() => {
		void version;
		const all = partial ? [...lines, cleanLine(partial)] : lines;
		const q = filter.trim().toLowerCase();
		return q ? all.filter((l) => l.toLowerCase().includes(q)) : all;
	});

	const first = $derived(Math.max(0, Math.floor(scrollTop / LINE_HEIGHT) - OVERSCAN));
	const last = $derived(
		Math.min(visibleLines.length, Math.ceil((scrollTop + viewport) / LINE_HEIGHT) + OVERSCAN)
	);

	$effect(() => {
		if (!scroller) return;
		const observer = new ResizeObserver(() => (viewport = scroller!.clientHeight));
		observer.observe(scroller);
		return () => observer.disconnect();
	});

	// Stick to the bottom while following.
	$effect(() => {
		void visibleLines.length;
		if (following && scroller) scroller.scrollTop = scroller.scrollHeight;
	});

	function onScroll(): void {
		if (!scroller) return;
		scrollTop = scroller.scrollTop;
		const atBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < LINE_HEIGHT * 2;
		following = atBottom;
	}

	function jumpToEnd(): void {
		following = true;
		if (scroller) scroller.scrollTop = scroller.scrollHeight;
	}

	async function copyAll(): Promise<void> {
		await copyText(visibleLines.join('\n'));
		toast.success('Copied to clipboard');
	}

	async function download(): Promise<void> {
		try {
			const path = await save({ defaultPath: downloadName });
			if (!path) return;
			await api.saveTextFile(path, visibleLines.join('\n') + '\n');
			toast.success('Saved');
		} catch (error) {
			toast.error(error, 'Could not save the file');
		}
	}

	const COLORS = { error: 'text-destructive', warn: 'text-warning', debug: 'text-muted-foreground' };
</script>

<div class={cn('bg-terminal flex min-h-0 flex-col overflow-hidden rounded-lg border', className)}>
	{#if controls}
		<div class="bg-card flex shrink-0 items-center gap-1.5 border-b px-2 py-1">
			{@render toolbar?.()}
			<SearchInput bind:value={filter} placeholder="Filter lines…" class="w-48" />
			<span class="text-muted-foreground tabular ml-1 text-xs">
				{visibleLines.length.toLocaleString()} line{visibleLines.length === 1 ? '' : 's'}
			</span>
			<div class="ml-auto flex items-center gap-0.5">
				{#if live}
					<IconButton label={paused ? 'Resume' : 'Pause'} onclick={togglePause}>
						{#if paused}<Play />{:else}<Pause />{/if}
					</IconButton>
					<IconButton label="Clear" onclick={() => reset()}><Eraser /></IconButton>
				{/if}
				{#if !following}
					<IconButton label="Jump to end and follow" onclick={jumpToEnd}><ArrowDownToLine /></IconButton>
				{/if}
				<IconButton label="Copy" onclick={copyAll}><Copy /></IconButton>
				<IconButton label="Download" onclick={download}><Download /></IconButton>
			</div>
		</div>
	{/if}
	<div bind:this={scroller} class="selectable min-h-0 flex-1 overflow-auto" onscroll={onScroll}>
		{#if visibleLines.length === 0}
			<p class="text-muted-foreground p-3 text-xs">{filter ? 'No lines match the filter.' : placeholder}</p>
		{:else}
			<div
				class="min-w-max px-3 py-2 font-mono text-xs"
				style="height: {visibleLines.length * LINE_HEIGHT + 16}px; line-height: {LINE_HEIGHT}px"
			>
				<div style="transform: translateY({first * LINE_HEIGHT}px)">
					{#each visibleLines.slice(first, last) as line, i (first + i)}
						{@const level = severity ? severityOf(line) : null}
						<div class={cn('whitespace-pre', level && COLORS[level])} style="height: {LINE_HEIGHT}px">
							{line || ' '}
						</div>
					{/each}
				</div>
			</div>
		{/if}
	</div>
</div>
