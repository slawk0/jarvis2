<script lang="ts">
	import Gauge from '@lucide/svelte/icons/gauge';
	import OctagonX from '@lucide/svelte/icons/octagon-x';
	import Skull from '@lucide/svelte/icons/skull';
	import DataTable, { type Column, type Sort } from '$lib/components/DataTable.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { api, type Process } from '$lib/ipc';
	import { confirm, prompt } from '$lib/services/confirm.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, resource } from '$lib/state/resource.svelte';
	import type { TabProps } from '$lib/workspace/registry';

	let { visible }: TabProps = $props();

	const processes = resource((io) => io.processList());
	let auto = $state(true);
	let search = $state('');
	let sort = $state<Sort | null>({ key: 'cpu', dir: 'desc' });
	let busy = $state(new Set<string>());

	autoLoad(
		processes,
		() => visible,
		3000,
		() => auto
	);

	const columns: Column<Process>[] = [
		{ key: 'pid', label: 'PID', value: (p) => p.pid, align: 'right', class: 'w-20 tabular' },
		{ key: 'user', label: 'User', value: (p) => p.user, class: 'w-32 max-w-32' },
		{ key: 'cpu', label: 'CPU %', value: (p) => p.cpu, align: 'right', class: 'w-20 tabular' },
		{ key: 'mem', label: 'MEM %', value: (p) => p.mem, align: 'right', class: 'w-20 tabular' },
		{ key: 'nice', label: 'Nice', value: (p) => p.nice, align: 'right', class: 'w-16 tabular' },
		{ key: 'command', label: 'Command', value: (p) => p.command, mono: true, class: 'max-w-0 w-full' }
	];

	async function withBusy(p: Process, action: () => Promise<unknown>, done: string): Promise<void> {
		const key = String(p.pid);
		busy = new Set(busy).add(key);
		try {
			await action();
			toast.success(done);
			await processes.load(true);
		} catch (error) {
			toast.error(error);
		} finally {
			const next = new Set(busy);
			next.delete(key);
			busy = next;
		}
	}

	const shortName = (p: Process) => p.command.split(' ')[0].split('/').pop() ?? String(p.pid);

	function terminate(p: Process): Promise<void> {
		return withBusy(
			p,
			() => sudo.describe(`Terminate process ${p.pid}`, () => api.processSignal(p.pid, 'term')),
			`Sent SIGTERM to ${shortName(p)} (${p.pid})`
		);
	}

	async function kill(p: Process): Promise<void> {
		const ok = await confirm({
			title: `Kill ${shortName(p)} (${p.pid})?`,
			message: 'SIGKILL ends the process immediately; it cannot clean up or save its state.',
			detail: p.command,
			confirmLabel: 'Kill process',
			destructive: true
		});
		if (!ok) return;
		await withBusy(
			p,
			() => sudo.describe(`Kill process ${p.pid}`, () => api.processSignal(p.pid, 'kill')),
			`Killed ${shortName(p)} (${p.pid})`
		);
	}

	async function renice(p: Process): Promise<void> {
		const value = await prompt({
			title: `Change priority of ${shortName(p)} (${p.pid})`,
			label: 'Nice value (−20 highest priority … 19 lowest)',
			value: String(p.nice ?? 0),
			validate: (v) =>
				/^-?\d+$/.test(v.trim()) && Number(v) >= -20 && Number(v) <= 19
					? null
					: 'Enter a whole number from -20 to 19.'
		});
		if (value === null) return;
		await withBusy(
			p,
			() => sudo.describe(`Renice process ${p.pid}`, () => api.processRenice(p.pid, Number(value))),
			`Priority of ${p.pid} set to ${Number(value)}`
		);
	}

	export const refresh = () => processes.refresh();
</script>

<Page scroll={false}>
	{#snippet toolbar()}
		<SearchInput bind:value={search} placeholder="Search command, user or PID…" class="w-72" />
		<span class="text-xs text-muted-foreground tabular">{processes.data?.length ?? 0} processes</span>
		<span class="flex-1"></span>
		<RefreshControl onrefresh={processes.refresh} loading={processes.loading} bind:auto />
	{/snippet}
	<DataTable
		rows={processes.data ?? []}
		{columns}
		rowKey={(p) => String(p.pid)}
		{search}
		bind:sort
		{busy}
		loading={processes.loading}
		error={processes.error}
		onretry={processes.refresh}
		empty="No processes"
		rowClass={(p) => (p.cpu >= 50 || p.mem >= 30 ? 'bg-warning/8' : undefined)}
		class="flex-1"
	>
		{#snippet cell(p, column)}
			{#if column.key === 'cpu'}
				<span class={p.cpu >= 50 ? 'font-medium text-warning' : ''}>{p.cpu.toFixed(1)}</span>
			{:else if column.key === 'mem'}
				<span class={p.mem >= 30 ? 'font-medium text-warning' : ''}>{p.mem.toFixed(1)}</span>
			{:else if column.key === 'nice'}
				{p.nice ?? '—'}
			{:else}
				{column.value?.(p)}
			{/if}
		{/snippet}
		{#snippet actions(p)}
			<IconButton label="Change priority (renice)" onclick={() => renice(p)}><Gauge /></IconButton>
			<IconButton label="Terminate (SIGTERM)" onclick={() => terminate(p)}><OctagonX /></IconButton>
			<IconButton label="Kill (SIGKILL)" onclick={() => kill(p)}><Skull /></IconButton>
		{/snippet}
		{#snippet menu(p)}
			<ContextMenu.Item onclick={() => terminate(p)}>Terminate (SIGTERM)</ContextMenu.Item>
			<ContextMenu.Item variant="destructive" onclick={() => kill(p)}>Kill (SIGKILL)</ContextMenu.Item>
			<ContextMenu.Item onclick={() => renice(p)}>Change priority…</ContextMenu.Item>
		{/snippet}
	</DataTable>
</Page>
