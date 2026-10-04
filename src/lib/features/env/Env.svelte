<script lang="ts">
	import Copy from '@lucide/svelte/icons/copy';
	import Eye from '@lucide/svelte/icons/eye';
	import EyeOff from '@lucide/svelte/icons/eye-off';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, toIpcError, type EnvFile, type EnvVar, type IpcError, type ManagedVar } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { resource } from '$lib/state/resource.svelte';
	import { copyText } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';

	let { visible }: TabProps = $props();

	type View = 'host' | 'persistent' | 'container';
	let view = $state<View>('host');
	let search = $state('');
	let revealed = $state(new Set<string>());
	const host = resource((io) => io.envHost());
	const managed = resource((io) => io.envManaged());
	let containers = $state<string[] | null>(null);
	let container = $state('');
	let containerVars = $state<EnvVar[]>([]);
	let containerError = $state<IpcError | null>(null);
	let containerLoading = $state(false);

	let formOpen = $state(false);
	let editing = $state<ManagedVar | null>(null);
	let form = $state({ file: 'bashrc' as EnvFile, key: '', value: '' });
	let saving = $state(false);

	const SECRET = /pass|secret|token|key|pwd|credential|api/i;
	const FILES: { value: EnvFile; label: string }[] = [
		{ value: 'bashrc', label: '~/.bashrc' },
		{ value: 'profile', label: '~/.profile' },
		{ value: 'bashProfile', label: '~/.bash_profile' },
		{ value: 'environment', label: '/etc/environment (all users, needs root)' }
	];

	const loaded = new Set<View>();
	$effect(() => {
		if (!visible || loaded.has(view)) return;
		loaded.add(view);
		if (view === 'host') void host.refresh();
		else if (view === 'persistent') void managed.refresh();
		else void loadContainers();
	});

	async function loadContainers(): Promise<void> {
		containerError = null;
		try {
			containers = (await api.dockerContainers()).map((c) => c.name);
			if (!container && containers.length) await pickContainer(containers[0]);
		} catch (raw) {
			containerError = toIpcError(raw);
			containers = [];
		}
	}

	async function pickContainer(name: string): Promise<void> {
		container = name;
		containerLoading = true;
		containerError = null;
		try {
			containerVars = await api.envContainer(name);
		} catch (raw) {
			containerError = toIpcError(raw);
		} finally {
			containerLoading = false;
		}
	}

	const columns: Column<EnvVar>[] = [
		{ key: 'key', label: 'Variable', value: (v) => v.key, mono: true, class: 'w-80 max-w-80' },
		{ key: 'value', label: 'Value', value: (v) => v.value, mono: true, class: 'max-w-0 w-full' }
	];
	const managedColumns: Column<ManagedVar>[] = [
		{ key: 'key', label: 'Variable', value: (v) => v.key, mono: true, class: 'w-72 max-w-72' },
		{ key: 'value', label: 'Value', value: (v) => v.value, mono: true, class: 'max-w-0 w-full' },
		{ key: 'path', label: 'File', value: (v) => v.path, mono: true, class: 'w-64 max-w-64' }
	];

	function toggleReveal(id: string): void {
		const next = new Set(revealed);
		if (next.has(id)) next.delete(id);
		else next.add(id);
		revealed = next;
	}

	function openForm(variable: ManagedVar | null): void {
		editing = variable;
		form = variable
			? { file: variable.file, key: variable.key, value: variable.value }
			: { file: 'bashrc', key: '', value: '' };
		formOpen = true;
	}

	const keyError = $derived(
		/^[A-Za-z_][A-Za-z0-9_]*$/.test(form.key) ? null : 'Letters, digits and “_”; must not start with a digit.'
	);

	async function save(): Promise<void> {
		if (keyError) return;
		saving = true;
		try {
			await sudo.describe(`Save ${form.key}`, () => api.envSet(form.file, form.key, form.value));
			toast.success(`${form.key} saved. It applies to new login sessions.`);
			formOpen = false;
			await managed.refresh();
		} catch (error) {
			toast.error(error, 'Could not save the variable');
		} finally {
			saving = false;
		}
	}

	async function remove(variable: ManagedVar): Promise<void> {
		const ok = await confirm({
			title: `Remove ${variable.key}?`,
			message: `It is removed from ${variable.path}.`,
			confirmLabel: 'Remove',
			destructive: true
		});
		if (!ok) return;
		try {
			await sudo.describe(`Remove ${variable.key}`, () => api.envRemove(variable.file, variable.key));
			await managed.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	export function refresh(): void {
		if (view === 'host') void host.refresh();
		else if (view === 'persistent') void managed.refresh();
		else void (container ? pickContainer(container) : loadContainers());
	}
</script>

{#snippet valueCell(id: string, key: string, value: string)}
	{@const secret = SECRET.test(key)}
	<span class="flex items-center gap-1">
		<span class="min-w-0 flex-1 truncate"
			>{secret && !revealed.has(id) ? '•'.repeat(Math.min(24, Math.max(8, value.length))) : value}</span
		>
		{#if secret}
			<IconButton
				label={revealed.has(id) ? 'Hide value' : 'Reveal value'}
				size="icon-xs"
				onclick={() => toggleReveal(id)}
			>
				{#if revealed.has(id)}<EyeOff />{:else}<Eye />{/if}
			</IconButton>
		{/if}
		<IconButton
			label="Copy value"
			size="icon-xs"
			onclick={() => copyText(value).then(() => toast.success('Value copied'))}><Copy /></IconButton
		>
	</span>
{/snippet}

<Page scroll={false}>
	{#snippet toolbar()}
		<SearchInput bind:value={search} placeholder="Search variables…" class="w-64" />
		{#if view === 'container' && containers?.length}
			<SelectField value={container} options={containers} class="w-64" onchange={pickContainer} />
		{/if}
		<span class="flex-1"></span>
		{#if view === 'persistent'}
			<Button size="sm" onclick={() => openForm(null)}><Plus /> Add variable</Button>
		{/if}
		<RefreshControl onrefresh={refresh} loading={host.loading || managed.loading || containerLoading} />
	{/snippet}
	{#snippet header()}
		<SubTabs
			bind:value={view}
			items={[
				{ id: 'host', label: 'Current environment' },
				{ id: 'persistent', label: 'Persistent variables', count: managed.data?.length },
				{ id: 'container', label: 'Docker container' }
			]}
		/>
	{/snippet}

	{#if view === 'host'}
		<DataTable
			rows={host.data ?? []}
			{columns}
			rowKey={(v) => v.key}
			{search}
			loading={host.loading}
			error={host.error}
			onretry={host.refresh}
			empty="No variables"
			class="flex-1"
		>
			{#snippet cell(v, column)}
				{#if column.key === 'value'}{@render valueCell(`h:${v.key}`, v.key, v.value)}{:else}{v.key}{/if}
			{/snippet}
		</DataTable>
	{:else if view === 'persistent'}
		<DataTable
			rows={managed.data ?? []}
			columns={managedColumns}
			rowKey={(v) => `${v.file}:${v.key}`}
			{search}
			loading={managed.loading}
			error={managed.error}
			onretry={managed.refresh}
			empty="No persistent variables yet"
			emptyHint="Variables added here are written to a profile file and marked so Jarvis can edit or remove them later."
			class="flex-1"
		>
			{#snippet cell(v, column)}
				{#if column.key === 'value'}{@render valueCell(
						`m:${v.file}:${v.key}`,
						v.key,
						v.value
					)}{:else}{column.value?.(v)}{/if}
			{/snippet}
			{#snippet actions(v)}
				<IconButton label="Edit" onclick={() => openForm(v)}><Pencil /></IconButton>
				<IconButton label="Remove" onclick={() => remove(v)}><Trash2 /></IconButton>
			{/snippet}
		</DataTable>
	{:else if containerError && !containers?.length}
		<StateView kind="error" error={containerError} onretry={loadContainers} />
	{:else if containers === null}
		<StateView kind="loading" />
	{:else if containers.length === 0}
		<StateView kind="empty" title="No containers" message="There are no Docker containers on this server." />
	{:else}
		<DataTable
			rows={containerVars}
			{columns}
			rowKey={(v) => v.key}
			{search}
			loading={containerLoading}
			error={containerError}
			onretry={() => pickContainer(container)}
			empty="This container defines no variables"
			class="flex-1"
		>
			{#snippet cell(v, column)}
				{#if column.key === 'value'}{@render valueCell(`c:${v.key}`, v.key, v.value)}{:else}{v.key}{/if}
			{/snippet}
		</DataTable>
	{/if}
</Page>

<Modal bind:open={formOpen} title={editing ? `Edit ${editing.key}` : 'Add persistent variable'} size="md">
	<form
		id="env-form"
		class="flex flex-col gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void save();
		}}
	>
		<Field label="File"
			><SelectField bind:value={form.file} options={FILES} disabled={editing !== null} /></Field
		>
		<Field label="Name" required error={form.key ? keyError : null}
			><Input
				bind:value={form.key}
				class="font-mono"
				spellcheck="false"
				disabled={editing !== null}
				autofocus
			/></Field
		>
		<Field label="Value"><Input bind:value={form.value} class="font-mono" spellcheck="false" /></Field>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (formOpen = false)}>Cancel</Button>
		<Button type="submit" form="env-form" disabled={saving || keyError !== null}>Save</Button>
	{/snippet}
</Modal>
