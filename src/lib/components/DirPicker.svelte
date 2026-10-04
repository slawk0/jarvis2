<!-- Modal for choosing a directory on the server. -->
<script lang="ts">
	import CornerLeftUp from '@lucide/svelte/icons/corner-left-up';
	import Folder from '@lucide/svelte/icons/folder';
	import { Button } from '$lib/components/ui/button';
	import { api, toIpcError, type FileEntry, type IpcError } from '$lib/ipc';
	import { compare } from '$lib/utils';
	import Modal from './Modal.svelte';
	import PathInput from './PathInput.svelte';
	import StateView from './StateView.svelte';

	interface Props {
		open: boolean;
		title?: string;
		/** Directory to start in; defaults to the home directory. */
		start?: string;
		confirmLabel?: string;
		onpick: (path: string) => void;
	}

	let {
		open = $bindable(false),
		title = 'Choose a folder',
		start,
		confirmLabel = 'Choose',
		onpick
	}: Props = $props();

	let path = $state('/');
	let typed = $state('/');
	let dirs = $state<FileEntry[]>([]);
	let loading = $state(false);
	let error = $state<IpcError | null>(null);

	async function go(target: string): Promise<void> {
		loading = true;
		error = null;
		try {
			const listing = await api.filesList(target || '/');
			path = listing.path;
			typed = listing.path;
			dirs = listing.entries.filter((e) => e.isDirLike).sort((a, b) => compare(a.name, b.name));
		} catch (raw) {
			error = toIpcError(raw);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (!open) return;
		void (async () => go(start || (await api.filesHome())))();
	});

	const parentPath = $derived(path.replace(/\/[^/]+\/?$/, '') || '/');
</script>

<Modal bind:open {title} size="md" class="h-[32rem]" flush>
	<div class="flex min-h-0 flex-1 flex-col gap-2">
		<PathInput bind:value={typed} dirsOnly onsubmit={go} />
		<div class="min-h-0 flex-1 overflow-y-auto rounded-lg border bg-card">
			{#if error}
				<StateView kind="error" {error} onretry={() => go(path)} compact />
			{:else if loading}
				<StateView kind="loading" compact />
			{:else}
				{#if path !== '/'}
					<button
						type="button"
						class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-muted"
						onclick={() => go(parentPath)}
					>
						<CornerLeftUp class="size-4 text-muted-foreground" /> ..
					</button>
				{/if}
				{#each dirs as dir (dir.name)}
					<button
						type="button"
						class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-muted"
						onclick={() => go(dir.path)}
					>
						<Folder class="size-4 shrink-0 text-primary" />
						<span class="truncate">{dir.name}</span>
					</button>
				{:else}
					<p class="p-3 text-xs text-muted-foreground">No subfolders.</p>
				{/each}
			{/if}
		</div>
	</div>
	{#snippet footer()}
		<span class="selectable mr-auto min-w-0 self-center truncate font-mono text-xs text-muted-foreground"
			>{path}</span
		>
		<Button variant="outline" onclick={() => (open = false)}>Cancel</Button>
		<Button
			disabled={loading || !!error}
			onclick={() => {
				open = false;
				onpick(path);
			}}
		>
			{confirmLabel}
		</Button>
	{/snippet}
</Modal>
