<!-- User devices (block, archive, delete) and resource access tokens (revoke). -->
<script lang="ts">
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import IpLink from '$lib/components/IpLink.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { formatDateTime, formatRelative } from '$lib/format';
	import { toIpcError, type IpcError } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { Busy } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';
	import { bool, listAll, orgPath, pangolin, pick, str, toDate, type Json } from './client';

	interface Props {
		visible: boolean;
	}

	let { visible }: Props = $props();

	let view = $state<'devices' | 'tokens'>('devices');
	let search = $state('');
	let loading = $state(false);
	let error = $state<IpcError | null>(null);
	let devices = $state<Json[]>([]);
	let tokens = $state<Json[]>([]);
	const busy = new Busy();

	async function load(): Promise<void> {
		loading = true;
		try {
			[devices, tokens] = await Promise.all([
				// Blocked and archived devices are hidden by default.
				listAll(orgPath('/user-devices'), ['devices', 'clients', 'userDevices'], {
					status: 'active,pending,blocked,archived'
				}).catch(() => listAll(orgPath('/user-devices'), ['devices', 'clients', 'userDevices'])),
				listAll(orgPath('/access-tokens'), ['accessTokens', 'tokens']).catch(() => [] as Json[])
			]);
			error = null;
		} catch (raw) {
			error = toIpcError(raw);
		} finally {
			loading = false;
		}
	}

	let started = false;
	$effect(() => {
		if (!visible || started) return;
		started = true;
		void load();
	});

	const id = (d: Json) => str(pick(d, 'clientId', 'id'));
	const blocked = (d: Json) => bool(d.blocked) || str(d.status) === 'blocked';
	const archived = (d: Json) => bool(d.archived) || str(d.status) === 'archived';
	const stateOf = (d: Json) =>
		blocked(d)
			? 'Blocked'
			: archived(d)
				? 'Archived'
				: str(pick(d, 'approvalState', 'status')) === 'pending'
					? 'Pending'
					: bool(d.online)
						? 'Online'
						: 'Offline';
	const lastSeen = (d: Json) => toDate(pick(d, 'lastSeen', 'lastPing', 'lastActive'));
	const address = (d: Json) => str(pick(d, 'subnet', 'ip', 'address')).split('/')[0];

	const deviceColumns: Column<Json>[] = [
		{ key: 'name', label: 'Name', value: (d) => str(d.name), class: 'w-56 max-w-56' },
		{
			key: 'owner',
			label: 'Owner',
			value: (d) => str(pick(d, 'userEmail', 'email', 'username', 'userId')),
			class: 'w-64 max-w-64'
		},
		{ key: 'ip', label: 'IP', value: address, class: 'w-40' },
		{ key: 'state', label: 'State', value: stateOf, class: 'w-28' },
		{
			key: 'agent',
			label: 'Agent',
			value: (d) =>
				[str(pick(d, 'agent', 'olmAgent')), str(pick(d, 'olmVersion', 'version'))].filter(Boolean).join(' '),
			class: 'w-40 max-w-40'
		},
		{
			key: 'lastSeen',
			label: 'Last seen',
			value: (d) => lastSeen(d)?.getTime() ?? 0,
			class: 'max-w-0 w-full tabular'
		}
	];
	const tokenColumns: Column<Json>[] = [
		{
			key: 'label',
			label: 'Label',
			value: (t) => str(pick(t, 'title', 'label', 'description')),
			class: 'w-72 max-w-72'
		},
		{
			key: 'prefix',
			label: 'Token ID',
			value: (t) => str(pick(t, 'accessTokenId', 'prefix')),
			mono: true,
			class: 'w-52 max-w-52'
		},
		{
			key: 'resource',
			label: 'Resource',
			value: (t) => str(pick(t, 'resourceName', 'resourceNiceId', 'resourceId')),
			class: 'w-56 max-w-56'
		},
		{
			key: 'expires',
			label: 'Expires',
			value: (t) => toDate(t.expiresAt)?.getTime() ?? Number.MAX_SAFE_INTEGER,
			class: 'max-w-0 w-full tabular'
		}
	];

	async function run(
		row: Json,
		action: 'block' | 'unblock' | 'archive' | 'unarchive',
		done: string
	): Promise<void> {
		await busy.run(
			id(row),
			async () => {
				await pangolin('POST', `/v1/client/${id(row)}/${action}`);
				toast.success(done);
				await load();
			},
			(e) => toast.error(e)
		);
	}

	async function remove(row: Json): Promise<void> {
		const ok = await confirm({
			title: `Delete the device “${str(row.name)}”?`,
			message: 'It is disconnected and has to be registered again to come back.',
			confirmLabel: 'Delete',
			destructive: true
		});
		if (!ok) return;
		await busy.run(
			id(row),
			async () => {
				await pangolin('DELETE', `/v1/client/${id(row)}`);
				toast.success('Device deleted');
				await load();
			},
			(e) => toast.error(e)
		);
	}

	async function revoke(row: Json): Promise<void> {
		const tokenId = str(row.accessTokenId);
		const ok = await confirm({
			title: `Revoke “${str(pick(row, 'title', 'label')) || tokenId}”?`,
			message: 'Links that use this token stop working immediately.',
			confirmLabel: 'Revoke',
			destructive: true
		});
		if (!ok) return;
		await busy.run(
			tokenId,
			async () => {
				await pangolin('DELETE', `/v1/access-token/${tokenId}`);
				toast.success('Token revoked');
				await load();
			},
			(e) => toast.error(e)
		);
	}

	const TONE: Record<string, string> = {
		Online: 'bg-success/15 text-success',
		Blocked: 'bg-destructive/15 text-destructive',
		Pending: 'bg-warning/15 text-warning'
	};

	export const refresh = load;
</script>

<div class="flex min-h-0 flex-1 flex-col gap-2">
	<div class="flex flex-wrap items-center gap-2">
		<SubTabs
			bind:value={view}
			items={[
				{ id: 'devices', label: 'Devices', count: devices.length },
				{ id: 'tokens', label: 'Access tokens', count: tokens.length }
			]}
		/>
		<span class="flex-1"></span>
		<SearchInput bind:value={search} class="w-56" />
		<RefreshControl onrefresh={load} {loading} />
	</div>

	{#if view === 'devices'}
		<DataTable
			rows={devices}
			columns={deviceColumns}
			rowKey={id}
			{search}
			{loading}
			{error}
			onretry={load}
			busy={busy.keys}
			empty="No user devices"
			class="flex-1"
		>
			{#snippet cell(d, column)}
				{#if column.key === 'ip'}<IpLink ip={address(d)} class="text-xs" />
				{:else if column.key === 'state'}
					<span class={cn('rounded px-1.5 py-0.5 text-[11px] font-medium', TONE[stateOf(d)] ?? 'bg-muted')}
						>{stateOf(d)}</span
					>
				{:else if column.key === 'lastSeen'}
					{@const seen = lastSeen(d)}
					{#if seen}<span title={formatDateTime(seen)}>{formatRelative(seen)}</span>{:else}<span
							class="text-muted-foreground">never</span
						>{/if}
				{:else}{column.value?.(d)}{/if}
			{/snippet}
			{#snippet actions(d)}
				{#if blocked(d)}
					<Button variant="ghost" size="xs" onclick={() => run(d, 'unblock', 'Device unblocked')}
						>Unblock</Button
					>
				{:else}
					<Button variant="ghost" size="xs" onclick={() => run(d, 'block', 'Device blocked')}>Block</Button>
				{/if}
				{#if archived(d)}
					<Button variant="ghost" size="xs" onclick={() => run(d, 'unarchive', 'Device restored')}
						>Restore</Button
					>
				{:else}
					<Button variant="ghost" size="xs" onclick={() => run(d, 'archive', 'Device archived')}
						>Archive</Button
					>
				{/if}
				<IconButton label="Delete device" onclick={() => remove(d)}><Trash2 /></IconButton>
			{/snippet}
		</DataTable>
	{:else}
		<DataTable
			rows={tokens}
			columns={tokenColumns}
			rowKey={(t) => str(t.accessTokenId)}
			{search}
			{loading}
			{error}
			onretry={load}
			busy={busy.keys}
			empty="No access tokens"
			emptyHint="Access tokens are shareable links to a public resource."
			class="flex-1"
		>
			{#snippet cell(t, column)}
				{#if column.key === 'expires'}
					{@const date = toDate(t.expiresAt)}
					{#if date}{formatDateTime(date)}{:else}<span class="text-muted-foreground">never</span>{/if}
				{:else}{column.value?.(t)}{/if}
			{/snippet}
			{#snippet actions(t)}
				<Button variant="ghost" size="xs" onclick={() => revoke(t)}>Revoke</Button>
			{/snippet}
		</DataTable>
	{/if}
</div>
