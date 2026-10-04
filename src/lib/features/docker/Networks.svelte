<script lang="ts">
	import Braces from '@lucide/svelte/icons/braces';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, type Network } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';

	let { visible, onchanged }: { visible: boolean; onchanged: () => void } = $props();

	const networks = resource((io) => io.dockerNetworks());
	const busy = new Busy();
	let search = $state('');
	let selected = $state(new Set<string>());
	let createOpen = $state(false);
	let name = $state('');
	let driver = $state('bridge');
	let inspect = $state<{ name: string; json: string } | null>(null);

	autoLoad(networks, () => visible);

	const BUILTIN = ['bridge', 'host', 'none'];
	const columns: Column<Network>[] = [
		{ key: 'name', label: 'Name', value: (n) => n.name, class: 'max-w-0 w-full' },
		{ key: 'driver', label: 'Driver', value: (n) => n.driver, class: 'w-32' },
		{ key: 'scope', label: 'Scope', value: (n) => n.scope, class: 'w-28' },
		{ key: 'id', label: 'ID', value: (n) => n.id.slice(0, 12), mono: true, class: 'w-32' }
	];

	async function remove(list: Network[]): Promise<void> {
		const names = list.map((n) => n.name).filter((n) => !BUILTIN.includes(n));
		if (names.length === 0) return;
		const ok = await confirm({
			title: names.length === 1 ? `Remove network “${names[0]}”?` : `Remove ${names.length} networks?`,
			detail: names.length > 1 ? names.join('\n') : undefined,
			confirmLabel: 'Remove',
			destructive: true
		});
		if (!ok) return;
		if (
			await busy.run(
				names,
				() => api.dockerNetworkRemove(names),
				(e) => toast.error(e)
			)
		)
			toast.success('Network removed');
		selected = new Set();
		await networks.refresh();
		onchanged();
	}

	async function create(): Promise<void> {
		try {
			await api.dockerNetworkCreate(name.trim(), driver);
			toast.success(`Network ${name.trim()} created`);
			createOpen = false;
			await networks.refresh();
			onchanged();
		} catch (error) {
			toast.error(error, 'Could not create the network');
		}
	}

	async function showInspect(network: Network): Promise<void> {
		try {
			inspect = { name: network.name, json: await api.dockerInspect('network', network.name) };
		} catch (error) {
			toast.error(error);
		}
	}

	export const refresh = () => networks.refresh();
	/** Quiet reload after something changed outside this view. */
	export const sync = () => networks.load(true);
</script>

<Page scroll={false}>
	{#snippet toolbar()}
		<SearchInput bind:value={search} placeholder="Search networks…" class="w-64" />
		<span class="flex-1"></span>
		<Button
			size="sm"
			onclick={() => {
				name = '';
				driver = 'bridge';
				createOpen = true;
			}}><Plus /> Create network</Button
		>
		<RefreshControl onrefresh={networks.refresh} loading={networks.loading} />
	{/snippet}
	<DataTable
		rows={networks.data ?? []}
		{columns}
		rowKey={(n) => n.name}
		{search}
		selectable
		bind:selected
		busy={busy.keys}
		loading={networks.loading}
		error={networks.error}
		onretry={networks.refresh}
		empty="No networks"
		class="flex-1"
	>
		{#snippet actions(network)}
			<IconButton label="Inspect" onclick={() => showInspect(network)}><Braces /></IconButton>
			<IconButton label="Remove" disabled={BUILTIN.includes(network.name)} onclick={() => remove([network])}
				><Trash2 /></IconButton
			>
		{/snippet}
		{#snippet bulk(rows)}
			<Button variant="destructive" size="xs" onclick={() => remove(rows)}>Remove</Button>
		{/snippet}
	</DataTable>
</Page>

<Modal bind:open={createOpen} title="Create network" size="sm">
	<form
		id="network-form"
		class="flex flex-col gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void create();
		}}
	>
		<Field label="Name" required><Input bind:value={name} spellcheck="false" autofocus /></Field>
		<Field label="Driver"
			><SelectField bind:value={driver} options={['bridge', 'overlay', 'macvlan', 'ipvlan']} /></Field
		>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
		<Button type="submit" form="network-form" disabled={!name.trim()}>Create</Button>
	{/snippet}
</Modal>

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
