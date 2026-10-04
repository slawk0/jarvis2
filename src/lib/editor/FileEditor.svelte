<!--
	The shared remote file editor: loads a file, shows saved / unsaved /
	saving / error status, saves with Ctrl+S (optionally automatically), and
	reads/writes through sudo transparently when permission is denied.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Save from '@lucide/svelte/icons/save';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import { Button } from '$lib/components/ui/button';
	import { Switch } from '$lib/components/ui/switch';
	import IconButton from '$lib/components/IconButton.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { api, toIpcError, type IpcError } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { cn, debounce } from '$lib/utils';
	import { usePane } from '$lib/workspace/workspace.svelte';
	import CodeEditor from './CodeEditor.svelte';

	interface Props {
		path: string;
		onclose: () => void;
		/** Custom source instead of the plain remote file (e.g. a file inside a volume). */
		read?: () => Promise<{ content: string; elevated: boolean }>;
		write?: (content: string) => Promise<boolean>;
		/** Runs after every successful save (e.g. "test and reload nginx"). */
		aftersave?: () => Promise<void>;
		language?: string;
		readonly?: boolean;
	}

	let { path, onclose, read, write, aftersave, language, readonly = false }: Props = $props();

	const pane = usePane();
	let content = $state('');
	let original = $state('');
	let loading = $state(true);
	let loadError = $state<IpcError | null>(null);
	let saving = $state(false);
	let saveError = $state<string | null>(null);
	let elevated = $state(false);
	let problems = $state(0);
	let autoSave = $state(false);
	let lang = $state('plaintext');

	const dirty = $derived(content !== original);
	const status = $derived(
		saving ? 'Saving…' : saveError ? 'Save failed' : dirty ? 'Unsaved changes' : 'Saved'
	);

	async function load(): Promise<void> {
		loading = true;
		loadError = null;
		try {
			const file = read ? await read() : await api.filesRead(path);
			content = file.content;
			original = file.content;
			elevated = file.elevated;
			lang = language ?? (await import('./monaco')).languageFor(path);
		} catch (raw) {
			loadError = toIpcError(raw);
		} finally {
			loading = false;
		}
	}

	async function save(): Promise<void> {
		if (saving || readonly || !dirty) return;
		saving = true;
		saveError = null;
		const text = content;
		try {
			const usedRoot = write ? await write(text) : await api.filesWrite(path, text);
			elevated = elevated || usedRoot;
			original = text;
			await aftersave?.();
		} catch (raw) {
			const error = toIpcError(raw);
			if (error.code !== 'CANCELLED') {
				saveError = error.message;
				toast.error(error, 'Could not save the file');
			}
		} finally {
			saving = false;
		}
	}

	const scheduleAutoSave = debounce(() => void save(), 1500);
	$effect(() => {
		void content;
		if (autoSave && untrack(() => dirty)) scheduleAutoSave();
	});

	async function close(): Promise<void> {
		if (dirty) {
			const discard = await confirm({
				title: 'Discard unsaved changes?',
				message: `${path} has changes that were not saved.`,
				confirmLabel: 'Discard',
				destructive: true
			});
			if (!discard) return;
		}
		onclose();
	}

	$effect(() => {
		void path;
		untrack(() => void load());
	});

	// Closing the pane asks first while there are unsaved changes; Back closes the editor.
	$effect(() => {
		pane?.setLive(`editor:${path}`, dirty ? `Unsaved changes in ${path}` : null);
		return () => pane?.setLive(`editor:${path}`, null);
	});
	$effect(() => pane?.pushBack('Close editor', () => void close()));
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex h-10 shrink-0 items-center gap-2 border-b bg-card px-2">
		<IconButton label="Close editor" onclick={close}><ArrowLeft /></IconButton>
		<span class="selectable min-w-0 truncate font-mono text-xs">{path}</span>
		{#if elevated}
			<span
				class="flex shrink-0 items-center gap-1 text-xs text-warning"
				title="This file is read and written as root"
			>
				<ShieldAlert class="size-3.5" /> root
			</span>
		{/if}
		{#if problems > 0}
			<span class="flex shrink-0 items-center gap-1 text-xs text-destructive">
				<TriangleAlert class="size-3.5" />
				{problems} syntax problem{problems === 1 ? '' : 's'}
			</span>
		{/if}
		<span class="flex-1"></span>
		{#if !readonly}
			<span
				class={cn(
					'shrink-0 text-xs',
					saveError ? 'text-destructive' : dirty ? 'text-warning' : 'text-muted-foreground'
				)}
				role="status"
			>
				{status}
			</span>
			<label class="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
				<Switch size="sm" bind:checked={autoSave} /> Auto-save
			</label>
			<Button size="sm" disabled={!dirty || saving} onclick={save}><Save /> Save</Button>
		{:else}
			<span class="text-xs text-muted-foreground">Read-only</span>
		{/if}
	</header>
	{#if loading}
		<StateView kind="loading" />
	{:else if loadError}
		<StateView kind="error" error={loadError} onretry={load} />
	{:else}
		<CodeEditor bind:value={content} bind:problems language={lang} {readonly} onsave={save} class="flex-1" />
	{/if}
</div>
