<!--
	The shared remote file browser. The Files tab uses it for the whole
	server; other features (Docker volumes) confine it to a root directory.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { join as joinLocal } from '@tauri-apps/api/path';
	import { getCurrentWebview } from '@tauri-apps/api/webview';
	import { open as pickLocal } from '@tauri-apps/plugin-dialog';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import Eye from '@lucide/svelte/icons/eye';
	import EyeOff from '@lucide/svelte/icons/eye-off';
	import FileIcon from '@lucide/svelte/icons/file';
	import FilePlus from '@lucide/svelte/icons/file-plus';
	import Folder from '@lucide/svelte/icons/folder';
	import FolderPlus from '@lucide/svelte/icons/folder-plus';
	import Link from '@lucide/svelte/icons/link';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import Star from '@lucide/svelte/icons/star';
	import Upload from '@lucide/svelte/icons/upload';
	import DataTable, { type Column, type Sort } from '$lib/components/DataTable.svelte';
	import DirPicker from '$lib/components/DirPicker.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Switch } from '$lib/components/ui/switch';
	import FileEditor from '$lib/editor/FileEditor.svelte';
	import { formatBytes, formatDateTime, formatMode, formatOctal } from '$lib/format';
	import { api, apiQuiet, toIpcError, type FileEntry, type IpcError, type Listing } from '$lib/ipc';
	import { confirm, prompt } from '$lib/services/confirm.svelte';
	import { loadDoc, saveDoc } from '$lib/services/profile-data';
	import { toast } from '$lib/services/toast.svelte';
	import { transfers } from '$lib/services/transfers.svelte';
	import { Busy } from '$lib/state/resource.svelte';
	import { cn, compare, copyText, debounce } from '$lib/utils';
	import { usePane } from '$lib/workspace/workspace.svelte';
	import FileDialogs, { type FileDialog } from './FileDialogs.svelte';

	interface Props {
		visible: boolean;
		/** Directory to open first; defaults to the home directory. */
		start?: string;
		/** Navigation cannot leave this directory. */
		jail?: string;
		bookmarks?: boolean;
	}

	let { visible, start, jail, bookmarks: showBookmarks = true }: Props = $props();

	const DEFAULT_BOOKMARKS = ['/etc/nginx', '/var/log', '/home'];
	const pane = usePane();
	const busy = new Busy();

	let listing = $state<Listing | null>(null);
	let error = $state<IpcError | null>(null);
	let loading = $state(false);
	let typedPath = $state('');
	let showHidden = $state(false);
	let filter = $state('');
	let recursive = $state(false);
	let searchResults = $state<Listing | null>(null);
	let searching = $state(false);
	let selected = $state(new Set<string>());
	let sort = $state<Sort | null>({ key: 'name', dir: 'asc' });
	let sizes = $state<Record<string, number>>({});
	let bookmarks = $state<string[]>(DEFAULT_BOOKMARKS);
	let editing = $state<string | null>(null);
	let dialog = $state<FileDialog | null>(null);
	let picker = $state<{ mode: 'move' | 'copy'; paths: string[] } | null>(null);
	let dropActive = $state(false);
	let dragRows = $state<{ paths: string[]; x: number; y: number; over: string | null } | null>(null);
	let seq = 0;

	const cwd = $derived(listing?.path ?? '');
	const atRoot = $derived(cwd === '/' || (jail !== undefined && cwd === jail));
	const inSearch = $derived(recursive && searchResults !== null);

	const rows = $derived.by(() => {
		const source = inSearch ? (searchResults?.entries ?? []) : (listing?.entries ?? []);
		return source.filter((e) => showHidden || !e.name.startsWith('.'));
	});
	/** Directories first, then by the chosen column (DataTable sorting is bypassed). */
	const ordered = $derived.by(() => {
		const key = sort?.key ?? 'name';
		const sign = sort?.dir === 'desc' ? -1 : 1;
		const value = (e: FileEntry) =>
			key === 'size'
				? e.isDirLike
					? (sizes[e.path] ?? -1)
					: e.size
				: key === 'modified'
					? e.modified
					: key === 'owner'
						? e.owner
						: key === 'mode'
							? e.mode
							: e.name;
		return [...rows].sort(
			(a, b) => Number(b.isDirLike) - Number(a.isDirLike) || sign * compare(value(a), value(b))
		);
	});
	const byPath = $derived(new Map(ordered.map((e) => [e.path, e])));
	const selection = $derived(ordered.filter((e) => selected.has(e.path)));

	const columns = $derived<Column<FileEntry>[]>([
		{ key: 'name', label: 'Name', sortable: false, class: 'max-w-0 w-full' },
		...(inSearch
			? [{ key: 'path', label: 'Path', sortable: false, mono: true, class: 'max-w-64' } as Column<FileEntry>]
			: []),
		{ key: 'size', label: 'Size', sortable: false, align: 'right', class: 'w-24 tabular' },
		{ key: 'mode', label: 'Permissions', sortable: false, mono: true, class: 'w-36' },
		{ key: 'owner', label: 'Owner', sortable: false, class: 'w-24 max-w-24' },
		{ key: 'group', label: 'Group', sortable: false, class: 'w-24 max-w-24' },
		{ key: 'modified', label: 'Modified', sortable: false, class: 'w-40 tabular' }
	]);
	const SORTS = [
		{ key: 'name', label: 'Name' },
		{ key: 'size', label: 'Size' },
		{ key: 'modified', label: 'Modified' },
		{ key: 'owner', label: 'Owner' },
		{ key: 'mode', label: 'Permissions' }
	];

	function inJail(path: string): boolean {
		return jail === undefined || path === jail || path.startsWith(`${jail.replace(/\/$/, '')}/`);
	}

	async function go(path: string): Promise<void> {
		if (!inJail(path)) path = jail!;
		const mine = ++seq;
		loading = true;
		error = null;
		try {
			const result = await api.filesList(path || '/');
			if (mine !== seq) return;
			listing = result;
			typedPath = result.path;
			selected = new Set();
			filter = '';
			searchResults = null;
			sizes = {};
			void loadSizes(result);
		} catch (raw) {
			if (mine !== seq) return;
			error = toIpcError(raw);
		} finally {
			if (mine === seq) loading = false;
		}
	}

	async function loadSizes(result: Listing): Promise<void> {
		const dirs = result.entries.filter((e) => e.kind === 'dir').slice(0, 80);
		if (dirs.length === 0) return;
		try {
			const found = await apiQuiet.filesSizes(dirs.map((d) => d.path));
			if (listing?.path === result.path) sizes = Object.fromEntries(found.map((s) => [s.path, s.bytes]));
		} catch {
			// Folder sizes are a nicety; the listing works without them.
		}
	}

	const reload = () => go(cwd || start || '/');
	const parentOf = (path: string) => path.replace(/\/[^/]+\/?$/, '') || '/';
	const up = () => go(parentOf(cwd));

	const runSearch = debounce(async (query: string) => {
		if (!recursive || query.trim().length < 2) {
			searchResults = null;
			return;
		}
		searching = true;
		try {
			searchResults = await api.filesSearch(cwd, query.trim());
		} catch (raw) {
			toast.error(raw, 'Search failed');
		} finally {
			searching = false;
		}
	}, 350);
	$effect(() => {
		const query = filter;
		void recursive;
		untrack(() => runSearch(query));
	});

	// ------------------------------------------------------------------ lifecycle
	let started = false;
	$effect(() => {
		if (!visible || started) return;
		started = true;
		void (async () => {
			await go(start ?? (await api.filesHome().catch(() => '/')));
			if (showBookmarks)
				bookmarks = await loadDoc('sftpBookmarks', DEFAULT_BOOKMARKS).catch(() => DEFAULT_BOOKMARKS);
		})();
	});

	// Refresh when a transfer batch finishes.
	$effect(() => transfers.onBatchDone(() => visible && !editing && void reload()));

	// Back: leave the editor first (it registers itself), then go to the parent folder.
	$effect(() => {
		if (!visible || editing || atRoot || !listing) return;
		return pane?.pushBack('Parent folder', () => void up());
	});

	// Files dragged in from the operating system.
	$effect(() => {
		if (!visible) return;
		const off = getCurrentWebview().onDragDropEvent((event) => {
			const payload = event.payload;
			if (payload.type === 'enter' || payload.type === 'over') dropActive = !editing;
			else if (payload.type === 'leave') dropActive = false;
			else if (payload.type === 'drop') {
				const accept = dropActive;
				dropActive = false;
				if (accept && payload.paths.length > 0) void uploadPaths(payload.paths);
			}
		});
		return () => {
			dropActive = false;
			void off.then((stop) => stop());
		};
	});

	// ------------------------------------------------------------------ actions
	const fail = (e: IpcError) => toast.error(e);
	const baseName = (path: string) =>
		path
			.replace(/[\\/]+$/, '')
			.split(/[\\/]/)
			.pop() ?? path;
	const remoteJoin = (dir: string, name: string) => `${dir.replace(/\/$/, '')}/${name}`;
	const rootNote = (elevated: unknown) => (elevated === true ? ' (as root)' : '');

	function open(entry: FileEntry): void {
		if (entry.isDirLike) void go(entry.path);
		else editing = entry.path;
	}

	async function uploadPaths(paths: string[]): Promise<void> {
		await transfers.enqueue(
			paths.map((local) => ({ kind: 'upload', local, remote: remoteJoin(cwd, baseName(local)) }))
		);
	}

	async function uploadPick(directory: boolean): Promise<void> {
		const picked = await pickLocal({
			multiple: !directory,
			directory,
			title: directory ? 'Upload a folder' : 'Upload files'
		});
		const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
		if (paths.length > 0) await uploadPaths(paths);
	}

	async function download(entries: FileEntry[]): Promise<void> {
		const folder = await pickLocal({
			directory: true,
			title: 'Download to…',
			defaultPath: await api.defaultDownloadDir()
		});
		if (typeof folder !== 'string') return;
		const items = await Promise.all(
			entries.map(async (e) => ({
				kind: 'download' as const,
				remote: e.path,
				local: await joinLocal(folder, e.name)
			}))
		);
		await transfers.enqueue(items);
	}

	async function create(directory: boolean): Promise<void> {
		const name = await prompt({
			title: directory ? 'New folder' : 'New file',
			label: 'Name',
			validate: (v) => (!v.trim() ? 'Enter a name.' : v.includes('/') ? 'A name cannot contain “/”.' : null)
		});
		if (!name) return;
		if (await busy.run('create', () => api.filesCreate(cwd, name.trim(), directory), fail)) await reload();
	}

	async function rename(entry: FileEntry): Promise<void> {
		const name = await prompt({
			title: `Rename “${entry.name}”`,
			label: 'New name',
			value: entry.name,
			validate: (v) => (!v.trim() ? 'Enter a name.' : v.includes('/') ? 'A name cannot contain “/”.' : null)
		});
		if (!name || name === entry.name) return;
		if (await busy.run(entry.path, () => api.filesRename(entry.path, name.trim()), fail)) await reload();
	}

	async function duplicate(entry: FileEntry): Promise<void> {
		const dot = entry.name.lastIndexOf('.');
		const suggestion =
			dot > 0 ? `${entry.name.slice(0, dot)} copy${entry.name.slice(dot)}` : `${entry.name} copy`;
		const name = await prompt({
			title: `Duplicate “${entry.name}”`,
			label: 'Name of the copy',
			value: suggestion
		});
		if (!name) return;
		if (await busy.run(entry.path, () => api.filesDuplicate(entry.path, name.trim()), fail)) await reload();
	}

	async function remove(entries: FileEntry[]): Promise<void> {
		if (entries.length === 0) return;
		const dirs = entries.filter((e) => e.kind === 'dir').length;
		const ok = await confirm({
			title: entries.length === 1 ? `Delete “${entries[0].name}”?` : `Delete ${entries.length} items?`,
			message:
				dirs > 0
					? 'Folders are deleted with everything inside them. This cannot be undone.'
					: 'This cannot be undone.',
			detail: entries.length > 1 ? entries.map((e) => e.path).join('\n') : undefined,
			confirmLabel: 'Delete',
			destructive: true
		});
		if (!ok) return;
		await transfers.enqueue(entries.map((e) => ({ kind: 'delete', remote: e.path })));
		selected = new Set();
	}

	async function moveOrCopy(mode: 'move' | 'copy', paths: string[], destination: string): Promise<void> {
		const sources = paths.filter(
			(p) => parentOf(p) !== destination && p !== destination && !destination.startsWith(`${p}/`)
		);
		if (sources.length === 0) {
			toast.info('Nothing to do: the items are already there');
			return;
		}
		await transfers.enqueue(sources.map((remote) => ({ kind: mode, remote, destination })));
		selected = new Set();
	}

	async function compress(entries: FileEntry[], format: 'tarGz' | 'zip'): Promise<void> {
		const ext = format === 'zip' ? '.zip' : '.tar.gz';
		const name = await prompt({
			title: 'Compress',
			label: 'Archive name',
			value: `${entries.length === 1 ? entries[0].name : 'archive'}${ext}`
		});
		if (!name) return;
		const ok = await busy.run(
			entries.map((e) => e.path),
			async () =>
				toast.success(
					`Archive created${rootNote(
						await api.filesCompress(
							cwd,
							entries.map((e) => e.name),
							name.trim(),
							format
						)
					)}`
				),
			fail
		);
		if (ok) await reload();
	}

	async function extract(entry: FileEntry): Promise<void> {
		const ok = await busy.run(
			entry.path,
			async () => toast.success(`Extracted${rootNote(await api.filesExtract(entry.path))}`),
			fail
		);
		if (ok) await reload();
	}

	const isArchive = (name: string) => /\.(zip|tar|tar\.gz|tgz|tar\.bz2|tbz2|tar\.xz|txz|gz)$/i.test(name);

	async function toggleBookmark(): Promise<void> {
		const next = bookmarks.includes(cwd) ? bookmarks.filter((b) => b !== cwd) : [...bookmarks, cwd];
		bookmarks = next;
		try {
			await saveDoc('sftpBookmarks', next);
		} catch (raw) {
			toast.error(raw, 'Could not save bookmarks');
		}
	}

	// ------------------------------------------------------------------ row dragging
	function onPointerDown(event: PointerEvent): void {
		if (event.button !== 0 || inSearch) return;
		const row = (event.target as HTMLElement).closest<HTMLElement>('tr[data-key]');
		const key = row?.dataset.key;
		if (!key || (event.target as HTMLElement).closest('button, [role=checkbox]')) return;
		const paths = selected.has(key) ? [...selected] : [key];
		const startX = event.clientX;
		const startY = event.clientY;
		const move = (e: PointerEvent) => {
			if (!dragRows && Math.hypot(e.clientX - startX, e.clientY - startY) < 8) return;
			const target =
				document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>('tr[data-key]')?.dataset.key ??
				null;
			const entry = target ? byPath.get(target) : undefined;
			dragRows = {
				paths,
				x: e.clientX,
				y: e.clientY,
				over: entry?.isDirLike && !paths.includes(entry.path) ? entry.path : null
			};
		};
		const end = () => {
			window.removeEventListener('pointermove', move);
			window.removeEventListener('pointerup', end);
			const drop = dragRows;
			dragRows = null;
			if (drop?.over) void moveOrCopy('move', drop.paths, drop.over);
		};
		window.addEventListener('pointermove', move);
		window.addEventListener('pointerup', end);
	}

	function onRowClick(entry: FileEntry, event: MouseEvent): void {
		if (event.ctrlKey || event.metaKey) {
			const next = new Set(selected);
			if (next.has(entry.path)) next.delete(entry.path);
			else next.add(entry.path);
			selected = next;
		} else if (event.shiftKey && selected.size > 0) {
			const paths = ordered.map((e) => e.path);
			const anchor = paths.indexOf([...selected][selected.size - 1]);
			const [a, b] = [anchor, paths.indexOf(entry.path)].sort((x, y) => x - y);
			selected = new Set([...selected, ...paths.slice(a, b + 1)]);
		} else {
			selected = new Set([entry.path]);
		}
	}

	export const refresh = reload;
	export function goTo(path: string): void {
		editing = null;
		void go(path);
	}
	export function closeEditor(): boolean {
		if (!editing) return false;
		editing = null;
		return true;
	}
</script>

{#if editing}
	{#key editing}
		<FileEditor
			path={editing}
			onclose={() => {
				editing = null;
				void reload();
			}}
		/>
	{/key}
{:else}
	<div class="relative flex h-full min-h-0 flex-col gap-2 p-3">
		<div class="flex flex-wrap items-center gap-1.5">
			<IconButton label="Parent folder" variant="outline" disabled={atRoot} onclick={up}
				><ArrowUp /></IconButton
			>
			<PathInput bind:value={typedPath} dirsOnly class="min-w-60 flex-1" onsubmit={go} />
			{#if listing?.elevated}
				<span
					class="flex items-center gap-1 text-xs text-warning"
					title="You cannot read this folder yourself; it is shown using root privileges"
				>
					<ShieldAlert class="size-3.5" /> as root
				</span>
			{/if}
			{#if showBookmarks}
				<DropdownMenu.Root>
					<DropdownMenu.Trigger>
						{#snippet child({ props })}
							<Button {...props} variant="outline" size="icon-sm" aria-label="Bookmarks">
								<Star class={bookmarks.includes(cwd) ? 'fill-warning text-warning' : ''} />
							</Button>
						{/snippet}
					</DropdownMenu.Trigger>
					<DropdownMenu.Content align="end" class="w-64">
						<DropdownMenu.Item onclick={toggleBookmark}>
							{bookmarks.includes(cwd) ? 'Remove bookmark for this folder' : 'Bookmark this folder'}
						</DropdownMenu.Item>
						<DropdownMenu.Separator />
						{#each bookmarks as bookmark (bookmark)}
							<DropdownMenu.Item class="font-mono text-xs" onclick={() => go(bookmark)}
								>{bookmark}</DropdownMenu.Item
							>
						{:else}
							<p class="px-2 py-1.5 text-xs text-muted-foreground">No bookmarks.</p>
						{/each}
					</DropdownMenu.Content>
				</DropdownMenu.Root>
			{/if}
			<RefreshControl onrefresh={reload} {loading} />
		</div>

		<div class="flex flex-wrap items-center gap-1.5">
			<SearchInput
				bind:value={filter}
				placeholder={recursive ? 'Search in subfolders…' : 'Filter this folder…'}
				class="w-60"
			/>
			<label class="flex items-center gap-1.5 text-xs text-muted-foreground">
				<Switch size="sm" bind:checked={recursive} /> Include subfolders
			</label>
			{#if searching}<span class="text-xs text-muted-foreground">Searching…</span>{/if}
			{#if inSearch && searchResults?.truncated}
				<span class="text-xs text-warning"
					>Showing the first {searchResults.entries.length} matches (truncated)</span
				>
			{/if}
			<span class="flex-1"></span>
			<DropdownMenu.Root>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<Button {...props} variant="ghost" size="sm"
							>Sort: {SORTS.find((s) => s.key === sort?.key)?.label}
							{sort?.dir === 'desc' ? '↓' : '↑'}</Button
						>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content align="end">
					{#each SORTS as option (option.key)}
						<DropdownMenu.Item
							onclick={() =>
								(sort = {
									key: option.key,
									dir: sort?.key === option.key && sort.dir === 'asc' ? 'desc' : 'asc'
								})}
						>
							{option.label}
						</DropdownMenu.Item>
					{/each}
				</DropdownMenu.Content>
			</DropdownMenu.Root>
			<IconButton
				label={showHidden ? 'Hide hidden files' : 'Show hidden files'}
				onclick={() => (showHidden = !showHidden)}
			>
				{#if showHidden}<Eye />{:else}<EyeOff />{/if}
			</IconButton>
			<IconButton label="New file" variant="outline" onclick={() => create(false)}><FilePlus /></IconButton>
			<IconButton label="New folder" variant="outline" onclick={() => create(true)}><FolderPlus /></IconButton
			>
			<DropdownMenu.Root>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<Button {...props} size="sm"><Upload /> Upload</Button>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content align="end">
					<DropdownMenu.Item onclick={() => uploadPick(false)}>Files…</DropdownMenu.Item>
					<DropdownMenu.Item onclick={() => uploadPick(true)}>Folder…</DropdownMenu.Item>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		</div>

		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="flex min-h-0 flex-1 flex-col" onpointerdown={onPointerDown}>
			<DataTable
				rows={ordered}
				{columns}
				rowKey={(e) => e.path}
				search={inSearch ? '' : filter}
				selectable
				bind:selected
				busy={busy.keys}
				{loading}
				{error}
				onretry={reload}
				empty="This folder is empty"
				emptyHint="Upload files or drop them here."
				onrowclick={onRowClick}
				onrowdblclick={open}
				rowClass={(e) => (dragRows?.over === e.path ? 'bg-primary/20 outline outline-primary' : undefined)}
				class="flex-1"
			>
				{#snippet cell(entry, column)}
					{#if column.key === 'name'}
						<span class="flex items-center gap-2">
							{#if entry.kind === 'symlink'}
								<Link class="size-4 shrink-0 text-info" />
							{:else if entry.isDirLike}
								<Folder class="size-4 shrink-0 text-primary" />
							{:else}
								<FileIcon class="size-4 shrink-0 text-muted-foreground" />
							{/if}
							<span class="truncate">{entry.name}</span>
						</span>
					{:else if column.key === 'path'}
						{parentOf(entry.path)}
					{:else if column.key === 'size'}
						{#if entry.kind === 'dir'}
							<span class="text-muted-foreground"
								>{sizes[entry.path] !== undefined ? formatBytes(sizes[entry.path]) : '—'}</span
							>
						{:else}
							{formatBytes(entry.size)}
						{/if}
					{:else if column.key === 'mode'}
						<span class="text-muted-foreground">{formatOctal(entry.mode)}</span> {formatMode(entry.mode)}
					{:else if column.key === 'owner'}
						{entry.owner}
					{:else if column.key === 'group'}
						{entry.group}
					{:else}
						{formatDateTime(entry.modified)}
					{/if}
				{/snippet}
				{#snippet bulk(entries)}
					<Button variant="outline" size="xs" onclick={() => download(entries)}>Download</Button>
					<Button
						variant="outline"
						size="xs"
						onclick={() => (picker = { mode: 'move', paths: entries.map((e) => e.path) })}>Move</Button
					>
					<Button
						variant="outline"
						size="xs"
						onclick={() => (picker = { mode: 'copy', paths: entries.map((e) => e.path) })}>Copy</Button
					>
					<Button variant="outline" size="xs" onclick={() => compress(entries, 'tarGz')}>Compress</Button>
					<Button variant="outline" size="xs" onclick={() => (dialog = { kind: 'permissions', entries })}
						>Permissions</Button
					>
					<Button variant="destructive" size="xs" onclick={() => remove(entries)}>Delete</Button>
				{/snippet}
				{#snippet menu(entry)}
					{@const targets = selected.has(entry.path) && selection.length > 1 ? selection : [entry]}
					<ContextMenu.Item onclick={() => open(entry)}>{entry.isDirLike ? 'Open' : 'Edit'}</ContextMenu.Item>
					<ContextMenu.Item onclick={() => download(targets)}>Download</ContextMenu.Item>
					<ContextMenu.Item onclick={() => copyText(entry.path).then(() => toast.success('Path copied'))}
						>Copy path</ContextMenu.Item
					>
					<ContextMenu.Separator />
					<ContextMenu.Item onclick={() => (picker = { mode: 'move', paths: targets.map((e) => e.path) })}
						>Move…</ContextMenu.Item
					>
					<ContextMenu.Item onclick={() => (picker = { mode: 'copy', paths: targets.map((e) => e.path) })}
						>Copy to…</ContextMenu.Item
					>
					<ContextMenu.Item onclick={() => duplicate(entry)}>Duplicate…</ContextMenu.Item>
					<ContextMenu.Item onclick={() => rename(entry)}>Rename…</ContextMenu.Item>
					<ContextMenu.Separator />
					<ContextMenu.Item onclick={() => compress(targets, 'tarGz')}>Compress to .tar.gz…</ContextMenu.Item>
					<ContextMenu.Item onclick={() => compress(targets, 'zip')}>Compress to .zip…</ContextMenu.Item>
					{#if isArchive(entry.name)}
						<ContextMenu.Item onclick={() => extract(entry)}>Extract here</ContextMenu.Item>
					{/if}
					<ContextMenu.Separator />
					<ContextMenu.Item onclick={() => (dialog = { kind: 'permissions', entries: targets })}
						>Permissions…</ContextMenu.Item
					>
					<ContextMenu.Item onclick={() => (dialog = { kind: 'owner', entries: targets })}
						>Owner / group…</ContextMenu.Item
					>
					<ContextMenu.Item onclick={() => (dialog = { kind: 'properties', entries: [entry] })}
						>Properties</ContextMenu.Item
					>
					<ContextMenu.Separator />
					<ContextMenu.Item variant="destructive" onclick={() => remove(targets)}>Delete</ContextMenu.Item>
				{/snippet}
			</DataTable>
		</div>

		{#if dropActive}
			<div
				class="pointer-events-none absolute inset-2 z-20 flex items-center justify-center rounded-xl border-2 border-dashed border-primary bg-primary/10"
			>
				<p class="rounded-lg border bg-popover px-4 py-2 text-sm font-medium shadow">
					Drop to upload to <span class="font-mono">{cwd}</span>
				</p>
			</div>
		{/if}
	</div>
{/if}

{#if dragRows}
	<div
		class={cn('pointer-events-none fixed z-[90] rounded-md border bg-popover px-2 py-1 text-xs shadow-lg')}
		style="left:{dragRows.x + 12}px;top:{dragRows.y + 12}px"
	>
		{dragRows.over
			? `Move ${dragRows.paths.length} item${dragRows.paths.length === 1 ? '' : 's'} to ${baseName(dragRows.over)}`
			: `${dragRows.paths.length} item${dragRows.paths.length === 1 ? '' : 's'}`}
	</div>
{/if}

<FileDialogs bind:dialog onchanged={reload} />

{#if picker}
	{@const request = picker}
	<DirPicker
		bind:open={
			() => true,
			(value) => {
				if (!value) picker = null;
			}
		}
		title={request.mode === 'move' ? 'Move to…' : 'Copy to…'}
		start={cwd}
		confirmLabel={request.mode === 'move' ? 'Move here' : 'Copy here'}
		onpick={(destination) => {
			picker = null;
			void moveOrCopy(request.mode, request.paths, destination);
		}}
	/>
{/if}
