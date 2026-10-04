<!-- Browse the files of one snapshot: breadcrumbs, search, preview and download. -->
<script lang="ts">
	import { open as pickFolder } from '@tauri-apps/plugin-dialog';
	import Copy from '@lucide/svelte/icons/copy';
	import Download from '@lucide/svelte/icons/download';
	import Eye from '@lucide/svelte/icons/eye';
	import FileIcon from '@lucide/svelte/icons/file';
	import Folder from '@lucide/svelte/icons/folder';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { formatBytes, formatDateTime, formatMode } from '$lib/format';
	import { api, toIpcError, type IpcError, type Snapshot, type SnapshotNode } from '$lib/ipc';
	import { toast } from '$lib/services/toast.svelte';
	import { transfers } from '$lib/services/transfers.svelte';
	import { copyText } from '$lib/utils';

	interface Props {
		repo: string;
		snapshot: Snapshot;
		onclose: () => void;
	}

	let { repo, snapshot, onclose }: Props = $props();

	const PREVIEW_LIMIT = 1024 * 1024;

	let path = $state('/');
	let nodes = $state<SnapshotNode[]>([]);
	let loading = $state(false);
	let error = $state<IpcError | null>(null);
	let query = $state('');
	/** Set while search results (instead of a folder) are shown. */
	let searched = $state('');
	let preview = $state<{ node: SnapshotNode; text: string } | null>(null);
	let previewing = $state('');
	let seq = 0;

	const crumbs = $derived(
		path
			.split('/')
			.filter(Boolean)
			.map((name, i, all) => ({ name, path: `/${all.slice(0, i + 1).join('/')}` }))
	);

	const columns: Column<SnapshotNode>[] = [
		{ key: 'name', label: 'Name', value: (n) => n.name, class: 'max-w-0 w-full' },
		{
			key: 'size',
			label: 'Size',
			value: (n) => (n.dir ? -1 : n.size),
			align: 'right',
			class: 'w-28 tabular'
		},
		{ key: 'mode', label: 'Permissions', value: (n) => n.mode, mono: true, class: 'w-32' },
		{ key: 'modified', label: 'Modified', value: (n) => n.modified, class: 'w-48 tabular' }
	];

	async function load(run: () => Promise<SnapshotNode[]>): Promise<void> {
		const current = ++seq;
		loading = true;
		error = null;
		try {
			const result = await run();
			if (current === seq) nodes = result;
		} catch (raw) {
			if (current === seq) error = toIpcError(raw);
		} finally {
			if (current === seq) loading = false;
		}
	}

	function open(target: string): void {
		path = target;
		searched = '';
		query = '';
		void load(() => api.resticLs(repo, snapshot.id, target));
	}

	function search(): void {
		const text = query.trim();
		if (!text) return open(path);
		searched = text;
		void load(() => api.resticFind(repo, snapshot.id, text));
	}

	// Start in the snapshot's own path when it has exactly one.
	// (That path may be a single file: then its parent folder is shown.)
	async function start(): Promise<void> {
		const only = snapshot.paths.length === 1 ? snapshot.paths[0].replace(/\/+$/, '') : '';
		if (!only) return open('/');
		path = only;
		await load(() => api.resticLs(repo, snapshot.id, only));
		if (nodes.length === 0 && !error) open(only.slice(0, only.lastIndexOf('/')) || '/');
	}
	void start();

	async function show(node: SnapshotNode): Promise<void> {
		if (node.size > PREVIEW_LIMIT) {
			toast.warning('This file is too large to preview (limit 1 MB). Download it instead.');
			return;
		}
		previewing = node.path;
		try {
			preview = { node, text: await api.resticPreview(repo, snapshot.id, node.path, node.size) };
		} catch (e) {
			const err = toIpcError(e);
			if (err.is('BINARY_FILE')) toast.warning('This is a binary file. Download it instead.');
			else toast.error(err, 'Could not read the file');
		} finally {
			previewing = '';
		}
	}

	async function download(node: SnapshotNode): Promise<void> {
		const dir = await pickFolder({ directory: true, title: `Download ${node.name} to…` });
		if (typeof dir !== 'string') return;
		try {
			await api.resticDownload(repo, snapshot.id, node.path, node.dir, dir);
			toast.info(node.dir ? `Extracting ${node.name} as a tar archive…` : `Extracting ${node.name}…`);
			transfers.panelOpen = true;
		} catch (e) {
			toast.error(e, 'Could not start the download');
		}
	}
</script>

<Modal
	open
	title="Snapshot {snapshot.shortId}"
	description="{formatDateTime(snapshot.time)} · {snapshot.hostname} · {snapshot.paths.join(', ')}"
	size="full"
	flush
	{onclose}
>
	<div class="flex min-h-0 flex-1 flex-col gap-2 p-4">
		<div class="flex items-center gap-2">
			<nav class="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto text-sm" aria-label="Path">
				<button type="button" class="rounded px-1.5 py-0.5 font-mono hover:bg-muted" onclick={() => open('/')}
					>/</button
				>
				{#each crumbs as crumb (crumb.path)}
					<button
						type="button"
						class="rounded px-1.5 py-0.5 font-mono whitespace-nowrap hover:bg-muted"
						onclick={() => open(crumb.path)}>{crumb.name}</button
					>
					<span class="text-muted-foreground">/</span>
				{/each}
			</nav>
			<form
				class="flex gap-2"
				onsubmit={(e) => {
					e.preventDefault();
					search();
				}}
			>
				<Input bind:value={query} placeholder="Search file names in this snapshot…" class="h-8 w-72" />
				<Button type="submit" variant="outline" size="sm">Search</Button>
			</form>
		</div>
		{#if searched}
			<p class="text-xs text-muted-foreground">
				Results for “{searched}”{nodes.length >= 500 ? ' (first 500)' : ''} ·
				<button type="button" class="text-primary hover:underline" onclick={() => open(path)}
					>back to folder</button
				>
			</p>
		{/if}
		{#if error}
			<StateView kind="error" {error} onretry={() => (searched ? search() : open(path))} />
		{:else}
			<DataTable
				rows={nodes}
				{columns}
				rowKey={(n) => n.path}
				{loading}
				empty={searched ? 'No file name matches' : 'This folder is empty'}
				onrowdblclick={(n) => (n.dir ? open(n.path) : show(n))}
				class="flex-1"
			>
				{#snippet cell(n, column)}
					{#if column.key === 'name'}
						<span class="flex items-center gap-2">
							{#if n.dir}<Folder class="size-4 shrink-0 text-primary" />{:else}<FileIcon
									class="size-4 shrink-0 text-muted-foreground"
								/>{/if}
							<span class="truncate" title={n.path}>{searched ? n.path : n.name}</span>
						</span>
					{:else if column.key === 'size'}{n.dir ? '' : formatBytes(n.size)}
					{:else if column.key === 'mode'}{formatMode(n.mode)}
					{:else}{n.modified ? formatDateTime(n.modified) : ''}{/if}
				{/snippet}
				{#snippet actions(n)}
					{#if !n.dir}
						<IconButton label="Preview" disabled={previewing === n.path} onclick={() => show(n)}
							><Eye /></IconButton
						>
					{/if}
					<IconButton label={n.dir ? 'Download folder as .tar' : 'Download'} onclick={() => download(n)}
						><Download /></IconButton
					>
				{/snippet}
			</DataTable>
		{/if}
	</div>
</Modal>

{#if preview}
	{@const shown = preview}
	<Modal
		open
		title={shown.node.name}
		description={shown.node.path}
		size="xl"
		class="h-[70vh]"
		flush
		onclose={() => (preview = null)}
	>
		<pre class="selectable min-h-0 flex-1 overflow-auto p-4 font-mono text-xs">{shown.text}</pre>
		{#snippet footer()}
			<Button variant="outline" onclick={() => copyText(shown.text).then(() => toast.success('Copied'))}
				><Copy /> Copy</Button
			>
			<Button variant="outline" onclick={() => download(shown.node)}><Download /> Download</Button>
			<Button onclick={() => (preview = null)}>Close</Button>
		{/snippet}
	</Modal>
{/if}
