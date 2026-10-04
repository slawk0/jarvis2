<script lang="ts">
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Braces from '@lucide/svelte/icons/braces';
	import FolderOpen from '@lucide/svelte/icons/folder-open';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import { Button } from '$lib/components/ui/button';
	import FileBrowser from '$lib/features/files/FileBrowser.svelte';
	import { api, type Volume } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';

	let { visible, onchanged }: { visible: boolean; onchanged: () => void } = $props();

	const volumes = resource((io) => io.dockerVolumes());
	const busy = new Busy();
	let search = $state('');
	let selected = $state(new Set<string>());
	let browsing = $state<Volume | null>(null);
	let inspect = $state<{ name: string; json: string } | null>(null);

	autoLoad(volumes, () => visible && !browsing);

	const columns: Column<Volume>[] = [
		{ key: 'name', label: 'Name', value: (v) => v.name, class: 'max-w-72' },
		{ key: 'driver', label: 'Driver', value: (v) => v.driver, class: 'w-28' },
		{
			key: 'mountpoint',
			label: 'Mount point',
			value: (v) => v.mountpoint,
			mono: true,
			class: 'max-w-0 w-full'
		}
	];

	async function remove(list: Volume[]): Promise<void> {
		const names = list.map((v) => v.name);
		const ok = await confirm({
			title: names.length === 1 ? `Remove volume “${names[0]}”?` : `Remove ${names.length} volumes?`,
			message: 'All data stored in the volume is deleted permanently.',
			detail: names.length > 1 ? names.join('\n') : undefined,
			acknowledge: 'I understand that the data in the volume cannot be recovered.',
			confirmLabel: 'Remove',
			destructive: true
		});
		if (!ok) return;
		if (
			await busy.run(
				names,
				() => api.dockerVolumeRemove(names),
				(e) => toast.error(e)
			)
		)
			toast.success('Volume removed');
		selected = new Set();
		await volumes.refresh();
		onchanged();
	}

	async function prune(): Promise<void> {
		const ok = await confirm({
			title: 'Remove all unused volumes?',
			message: 'Every volume that no container uses is deleted together with its data.',
			acknowledge: 'I understand that the data in these volumes cannot be recovered.',
			confirmLabel: 'Prune volumes',
			destructive: true
		});
		if (!ok) return;
		try {
			const summary = await api.dockerPrune('volumes');
			toast.success(summary.trim().split('\n').pop() ?? 'Unused volumes removed');
			await volumes.refresh();
			onchanged();
		} catch (error) {
			toast.error(error);
		}
	}

	async function showInspect(volume: Volume): Promise<void> {
		try {
			inspect = { name: volume.name, json: await api.dockerInspect('volume', volume.name) };
		} catch (error) {
			toast.error(error);
		}
	}

	export const refresh = () => volumes.refresh();
	/** Quiet reload after something changed outside this view. */
	export const sync = () => volumes.load(true);
	export function reset(): void {
		browsing = null;
	}
</script>

{#if browsing}
	<div class="flex h-full min-h-0 flex-col">
		<div class="flex shrink-0 items-center gap-2 px-3 pt-3">
			<IconButton label="Back to volumes" variant="outline" onclick={() => (browsing = null)}
				><ArrowLeft /></IconButton
			>
			<h2 class="truncate text-[15px] font-semibold">Volume · {browsing.name}</h2>
			<span class="text-xs text-muted-foreground">Files are read and written as root.</span>
		</div>
		<div class="min-h-0 flex-1">
			<FileBrowser {visible} start={browsing.mountpoint} jail={browsing.mountpoint} bookmarks={false} />
		</div>
	</div>
{:else}
	<Page scroll={false}>
		{#snippet toolbar()}
			<SearchInput bind:value={search} placeholder="Search volumes…" class="w-64" />
			<span class="flex-1"></span>
			<Button variant="outline" size="sm" onclick={prune}>Prune unused</Button>
			<RefreshControl onrefresh={volumes.refresh} loading={volumes.loading} />
		{/snippet}
		<DataTable
			rows={volumes.data ?? []}
			{columns}
			rowKey={(v) => v.name}
			{search}
			selectable
			bind:selected
			busy={busy.keys}
			loading={volumes.loading}
			error={volumes.error}
			onretry={volumes.refresh}
			empty="No volumes"
			class="flex-1"
		>
			{#snippet cell(volume, column)}
				{#if column.key === 'name'}
					<span class="flex items-center gap-2">
						<span class="truncate font-medium">{volume.name}</span>
						{#if volume.unused}<span class="shrink-0 rounded bg-muted px-1 text-[10px] text-muted-foreground"
								>unused</span
							>{/if}
					</span>
				{:else}
					{column.value?.(volume)}
				{/if}
			{/snippet}
			{#snippet actions(volume)}
				<IconButton
					label="Browse files"
					disabled={!volume.mountpoint.startsWith('/')}
					onclick={() => (browsing = volume)}><FolderOpen /></IconButton
				>
				<IconButton label="Inspect" onclick={() => showInspect(volume)}><Braces /></IconButton>
				<IconButton label="Remove" onclick={() => remove([volume])}><Trash2 /></IconButton>
			{/snippet}
			{#snippet bulk(rows)}
				<Button variant="destructive" size="xs" onclick={() => remove(rows)}>Remove</Button>
			{/snippet}
		</DataTable>
	</Page>
{/if}

{#if inspect}
	<Modal
		open
		title="Inspect · {inspect.name}"
		size="xl"
		class="h-[75vh]"
		flush
		onclose={() => (inspect = null)}
	>
		<LogViewer source={inspect.json} downloadName="{inspect.name}.json" class="flex-1" />
	</Modal>
{/if}
