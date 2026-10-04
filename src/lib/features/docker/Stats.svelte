<script lang="ts">
	import DataTable, { type Column, type Sort } from '$lib/components/DataTable.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import { formatBytes } from '$lib/format';
	import type { ContainerStats } from '$lib/ipc';
	import { autoLoad, resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';

	// `onchanged` is part of the shared sub-tab contract; stats change nothing.
	// eslint-disable-next-line svelte/no-unused-props
	let { visible }: { visible: boolean; onchanged: () => void } = $props();

	const stats = resource((io) => io.dockerStats());
	let search = $state('');
	let sort = $state<Sort | null>({ key: 'cpu', dir: 'desc' });
	let auto = $state(true);
	let interval = $state(3);

	autoLoad(
		stats,
		() => visible,
		() => interval * 1000,
		() => auto
	);

	const columns: Column<ContainerStats>[] = [
		{ key: 'name', label: 'Container', value: (s) => s.name, class: 'max-w-0 w-full' },
		{ key: 'cpu', label: 'CPU', value: (s) => s.cpuPercent, class: 'w-48' },
		{ key: 'mem', label: 'Memory', value: (s) => s.memUsed, class: 'w-72' },
		{ key: 'net', label: 'Net I/O', value: (s) => s.netRx + s.netTx, align: 'right', class: 'w-44 tabular' },
		{
			key: 'block',
			label: 'Block I/O',
			value: (s) => s.blockRead + s.blockWrite,
			align: 'right',
			class: 'w-44 tabular'
		},
		{ key: 'pids', label: 'PIDs', value: (s) => s.pids, align: 'right', class: 'w-20 tabular' }
	];

	const tone = (p: number) => (p > 85 ? 'bg-destructive' : p > 65 ? 'bg-warning' : 'bg-primary');

	export const refresh = () => stats.refresh();
	/** Quiet reload after something changed outside this view. */
	export const sync = () => stats.load(true);
</script>

{#snippet bar(percent: number, label: string)}
	<div class="flex items-center gap-2">
		<div class="h-1.5 w-20 shrink-0 overflow-hidden rounded-full bg-muted">
			<div class={cn('h-full rounded-full', tone(percent))} style="width: {Math.min(100, percent)}%"></div>
		</div>
		<span class="text-xs tabular">{label}</span>
	</div>
{/snippet}

<Page scroll={false}>
	{#snippet toolbar()}
		<SearchInput bind:value={search} placeholder="Search containers…" class="w-64" />
		<span class="flex-1"></span>
		<RefreshControl
			onrefresh={stats.refresh}
			loading={stats.loading}
			bind:auto
			bind:interval
			intervals={[1, 2, 3, 5, 10]}
		/>
	{/snippet}
	<DataTable
		rows={stats.data ?? []}
		{columns}
		rowKey={(s) => s.id || s.name}
		{search}
		bind:sort
		loading={stats.loading}
		error={stats.error}
		onretry={stats.refresh}
		empty="No running containers"
		class="flex-1"
	>
		{#snippet cell(s, column)}
			{#if column.key === 'name'}
				<span class="font-medium">{s.name}</span>
			{:else if column.key === 'cpu'}
				{@render bar(s.cpuPercent, `${s.cpuPercent.toFixed(1)}%`)}
			{:else if column.key === 'mem'}
				{@render bar(s.memPercent, `${formatBytes(s.memUsed)} / ${formatBytes(s.memLimit)}`)}
			{:else if column.key === 'net'}
				↓ {formatBytes(s.netRx)} · ↑ {formatBytes(s.netTx)}
			{:else if column.key === 'block'}
				R {formatBytes(s.blockRead)} · W {formatBytes(s.blockWrite)}
			{:else}
				{s.pids}
			{/if}
		{/snippet}
	</DataTable>
</Page>
