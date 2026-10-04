<script lang="ts">
	import Info from '@lucide/svelte/icons/info';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import DataTable, { type Column, type Sort } from '$lib/components/DataTable.svelte';
	import IpLink from '$lib/components/IpLink.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { api, type Connection, type Interface, type Listener } from '$lib/ipc';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { poll, resource } from '$lib/state/resource.svelte';
	import type { TabProps } from '$lib/workspace/registry';
	import { workspace } from '$lib/workspace/workspace.svelte';

	let { visible }: TabProps = $props();

	type View = 'listening' | 'connections' | 'interfaces';
	let view = $state<View>('listening');
	let search = $state('');
	let elevated = $state(false);
	let listenSort = $state<Sort | null>({ key: 'port', dir: 'asc' });
	const listening = resource((io) => io.networkListening(elevated));
	const connections = resource((io) => io.networkConnections());
	const interfaces = resource((io) => io.networkInterfaces());
	const current = $derived({ listening, connections, interfaces }[view]);

	const loaded = new Set<View>();
	$effect(() => {
		if (!visible || loaded.has(view)) return;
		loaded.add(view);
		void current.refresh();
	});
	poll(
		() => visible && view !== 'interfaces',
		10_000,
		() => current.load(true)
	);

	const hiddenProcesses = $derived((listening.data ?? []).some((l) => !l.process));

	async function elevate(): Promise<void> {
		// Ask for the password now; the listing itself then runs as root.
		const granted =
			(await api.sudoStatus()).ready || (await sudo.request({ action: 'show the processes of all users' }));
		if (!granted) return;
		elevated = true;
		try {
			await listening.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	const listenColumns: Column<Listener>[] = [
		{ key: 'protocol', label: 'Protocol', value: (l) => l.protocol, class: 'w-24' },
		{ key: 'address', label: 'Local address', value: (l) => l.address, class: 'w-64 max-w-64' },
		{ key: 'port', label: 'Port', value: (l) => Number(l.port) || 0, align: 'right', class: 'w-24 tabular' },
		{ key: 'process', label: 'Process', value: (l) => l.process, class: 'max-w-0 w-full' },
		{ key: 'pid', label: 'PID', value: (l) => l.pid, align: 'right', class: 'w-24 tabular' }
	];
	const connColumns: Column<Connection>[] = [
		{ key: 'state', label: 'State', value: (c) => c.state, class: 'w-32' },
		{ key: 'local', label: 'Local', value: (c) => c.local, class: 'w-80 max-w-80' },
		{ key: 'remote', label: 'Remote', value: (c) => c.remote, class: 'max-w-0 w-full' }
	];
	const ifaceColumns: Column<Interface>[] = [
		{ key: 'name', label: 'Interface', value: (i) => i.name, class: 'w-48' },
		{ key: 'addresses', label: 'Addresses', value: (i) => i.addresses.join(', '), class: 'max-w-0 w-full' }
	];

	export const refresh = () => current.refresh();
</script>

<Page scroll={false}>
	{#snippet toolbar()}
		<SearchInput bind:value={search} placeholder="Filter…" class="w-56" />
		{#if view === 'listening' && hiddenProcesses && !elevated}
			<Button variant="outline" size="sm" onclick={elevate}><ShieldCheck /> Show all processes</Button>
		{/if}
		<p class="flex items-center gap-1.5 text-xs text-muted-foreground">
			<Info class="size-3.5" /> Open or close ports in the
			<button type="button" class="text-primary hover:underline" onclick={() => workspace.openTab('firewall')}
				>Firewall</button
			> tab.
		</p>
		<span class="flex-1"></span>
		<RefreshControl onrefresh={current.refresh} loading={current.loading} />
	{/snippet}
	{#snippet header()}
		<SubTabs
			bind:value={view}
			items={[
				{ id: 'listening', label: 'Listening', count: listening.data?.length },
				{ id: 'connections', label: 'Connections', count: connections.data?.length },
				{ id: 'interfaces', label: 'Interfaces' }
			]}
		/>
	{/snippet}

	{#if view === 'listening'}
		<DataTable
			rows={listening.data ?? []}
			columns={listenColumns}
			rowKey={(l) => `${l.protocol}|${l.address}|${l.port}|${l.pid}`}
			{search}
			bind:sort={listenSort}
			loading={listening.loading}
			error={listening.error}
			onretry={listening.refresh}
			empty="Nothing is listening"
			class="flex-1"
		>
			{#snippet cell(l, column)}
				{#if column.key === 'address'}
					<IpLink ip={l.address} class="text-xs" />
				{:else if column.key === 'port'}
					{l.port}
				{:else if column.key === 'process'}
					{#if l.process}{l.process}{:else}<span class="text-muted-foreground">owned by another user</span
						>{/if}
				{:else if column.key === 'pid'}
					{l.pid ?? '—'}
				{:else}
					{l.protocol}
				{/if}
			{/snippet}
		</DataTable>
	{:else if view === 'connections'}
		<DataTable
			rows={connections.data ?? []}
			columns={connColumns}
			rowKey={(c) => `${c.local}|${c.remote}`}
			{search}
			loading={connections.loading}
			error={connections.error}
			onretry={connections.refresh}
			empty="No established connections"
			class="flex-1"
		>
			{#snippet cell(c, column)}
				{#if column.key === 'local'}<IpLink ip={c.local} class="text-xs" />
				{:else if column.key === 'remote'}<IpLink ip={c.remote} class="text-xs" />
				{:else}{c.state}{/if}
			{/snippet}
		</DataTable>
	{:else}
		<DataTable
			rows={interfaces.data ?? []}
			columns={ifaceColumns}
			rowKey={(i) => i.name}
			{search}
			loading={interfaces.loading}
			error={interfaces.error}
			onretry={interfaces.refresh}
			empty="No interfaces reported"
			class="flex-1"
		>
			{#snippet cell(i, column)}
				{#if column.key === 'addresses'}
					<span class="flex flex-wrap gap-x-3"
						>{#each i.addresses as address (address)}<IpLink ip={address} class="text-xs" />{/each}</span
					>
				{:else}
					<span class="font-mono text-xs">{i.name}</span>
				{/if}
			{/snippet}
		</DataTable>
	{/if}
</Page>
