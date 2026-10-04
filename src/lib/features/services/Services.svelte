<script lang="ts">
	import FileText from '@lucide/svelte/icons/file-text';
	import Play from '@lucide/svelte/icons/play';
	import Plus from '@lucide/svelte/icons/plus';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import Square from '@lucide/svelte/icons/square';
	import DataTable, { type Column, type Sort } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { api, type RestartPolicy, type ServiceUnit, type UnitAction } from '$lib/ipc';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';
	import UnitDetail from './UnitDetail.svelte';

	let { visible }: TabProps = $props();

	const services = resource((io) => io.servicesList());
	const busy = new Busy();
	let search = $state('');
	let sort = $state<Sort | null>({ key: 'name', dir: 'asc' });
	let onlyRunning = $state(false);
	let detail = $state<string | null>(null);
	let createOpen = $state(false);
	let submitted = $state(false);
	let creating = $state(false);
	let form = $state(blank());

	autoLoad(services, () => visible && !detail, 15_000);

	function blank() {
		return {
			name: '',
			description: '',
			execStart: '',
			user: '',
			restart: 'onFailure' as RestartPolicy,
			workingDir: '',
			environment: '',
			enable: true,
			start: true
		};
	}

	const rows = $derived((services.data ?? []).filter((s) => !onlyRunning || s.active === 'active'));

	const columns: Column<ServiceUnit>[] = [
		{ key: 'name', label: 'Service', value: (s) => s.name, class: 'max-w-72' },
		{ key: 'active', label: 'State', value: (s) => `${s.active} ${s.sub}`, class: 'w-40' },
		{ key: 'enabled', label: 'On boot', value: (s) => s.enabled, class: 'w-28' },
		{ key: 'load', label: 'Load', value: (s) => s.load, class: 'w-24' },
		{ key: 'description', label: 'Description', value: (s) => s.description, class: 'max-w-0 w-full' }
	];

	const TONE: Record<string, string> = {
		active: 'bg-success',
		failed: 'bg-destructive',
		activating: 'bg-warning',
		deactivating: 'bg-warning'
	};

	const PAST: Record<UnitAction, string> = {
		start: 'started',
		stop: 'stopped',
		restart: 'restarted',
		reload: 'reloaded',
		enable: 'enabled',
		disable: 'disabled'
	};

	async function act(unit: ServiceUnit, action: UnitAction): Promise<void> {
		const ok = await busy.run(
			unit.name,
			() => sudo.describe(`${action} ${unit.name}`, () => api.unitAction(unit.name, action)),
			(e) => toast.error(e, `Could not ${action} ${unit.name}`)
		);
		if (ok) toast.success(`${unit.name} ${PAST[action]}`);
		await services.load(true);
	}

	const errors = $derived({
		name: !/^[A-Za-z0-9_.@-]+$/.test(form.name) ? 'Use letters, digits, “-”, “_” or “.”.' : null,
		execStart: !form.execStart.trim()
			? 'Enter the command to run.'
			: !form.execStart.trim().startsWith('/')
				? 'Use an absolute path to the program.'
				: null
	});

	async function create(): Promise<void> {
		submitted = true;
		if (errors.name || errors.execStart) return;
		creating = true;
		try {
			const environment = form.environment
				.split('\n')
				.map((l) => l.trim())
				.filter(Boolean)
				.map((l) => {
					const i = l.indexOf('=');
					return [l.slice(0, i), l.slice(i + 1)] as [string, string];
				});
			await sudo.describe(`Create service ${form.name}`, () =>
				api.serviceCreate({
					name: form.name,
					description: form.description,
					execStart: form.execStart,
					user: form.user,
					restart: form.restart,
					workingDir: form.workingDir,
					environment,
					enable: form.enable,
					start: form.start
				})
			);
			toast.success(`Service ${form.name} created`);
			createOpen = false;
			await services.refresh();
		} catch (error) {
			toast.error(error, 'Could not create the service');
		} finally {
			creating = false;
		}
	}

	export const refresh = () => services.refresh();
	export function onReselect(): void {
		detail = null;
	}
</script>

{#if detail}
	{#key detail}
		<UnitDetail
			unit={detail}
			{visible}
			onclose={() => {
				detail = null;
				void services.load(true);
			}}
		/>
	{/key}
{:else}
	<Page scroll={false}>
		{#snippet toolbar()}
			<SearchInput bind:value={search} placeholder="Search services…" class="w-64" />
			<label class="flex items-center gap-1.5 text-xs text-muted-foreground">
				<Checkbox bind:checked={onlyRunning} /> Active only
			</label>
			<span class="text-xs text-muted-foreground tabular">{rows.length} services</span>
			<span class="flex-1"></span>
			<Button
				size="sm"
				onclick={() => {
					form = blank();
					submitted = false;
					createOpen = true;
				}}><Plus /> Create service</Button
			>
			<RefreshControl onrefresh={services.refresh} loading={services.loading} />
		{/snippet}
		<DataTable
			{rows}
			{columns}
			rowKey={(s) => s.name}
			{search}
			bind:sort
			busy={busy.keys}
			loading={services.loading}
			error={services.error}
			onretry={services.refresh}
			empty="No services"
			onrowdblclick={(s) => (detail = s.name)}
			class="flex-1"
		>
			{#snippet cell(s, column)}
				{#if column.key === 'active'}
					<span class="flex items-center gap-2">
						<span class={cn('size-2 rounded-full', TONE[s.active] ?? 'bg-muted-foreground/40')}></span>
						{s.active} <span class="text-muted-foreground">({s.sub})</span>
					</span>
				{:else if column.key === 'enabled'}
					<span class={s.enabled === 'enabled' ? 'text-success' : 'text-muted-foreground'}
						>{s.enabled || '—'}</span
					>
				{:else if column.key === 'name'}
					<button
						type="button"
						class="truncate text-left font-medium hover:text-primary"
						onclick={() => (detail = s.name)}
					>
						{s.name.replace(/\.service$/, '')}
					</button>
				{:else}
					{column.value?.(s)}
				{/if}
			{/snippet}
			{#snippet actions(s)}
				{#if s.active === 'active' || s.active === 'activating'}
					<IconButton label="Restart" onclick={() => act(s, 'restart')}><RotateCw /></IconButton>
					<IconButton label="Stop" onclick={() => act(s, 'stop')}><Square /></IconButton>
				{:else}
					<IconButton label="Start" onclick={() => act(s, 'start')}><Play /></IconButton>
				{/if}
				<IconButton label="Status, logs and unit file" onclick={() => (detail = s.name)}
					><FileText /></IconButton
				>
			{/snippet}
			{#snippet menu(s)}
				<ContextMenu.Item onclick={() => act(s, 'start')}>Start</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act(s, 'stop')}>Stop</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act(s, 'restart')}>Restart</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act(s, 'reload')}>Reload</ContextMenu.Item>
				<ContextMenu.Separator />
				<ContextMenu.Item onclick={() => act(s, 'enable')}>Enable on boot</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act(s, 'disable')}>Disable on boot</ContextMenu.Item>
				<ContextMenu.Separator />
				<ContextMenu.Item onclick={() => (detail = s.name)}>Status, logs and unit file</ContextMenu.Item>
			{/snippet}
		</DataTable>
	</Page>
{/if}

<Modal
	bind:open={createOpen}
	title="Create service"
	description="Writes a unit to /etc/systemd/system and reloads systemd."
	size="lg"
>
	<form
		id="service-form"
		class="grid grid-cols-2 gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void create();
		}}
	>
		<Field label="Name" required error={submitted ? errors.name : null} hint="Becomes <name>.service">
			<Input bind:value={form.name} placeholder="myapp" spellcheck="false" autofocus />
		</Field>
		<Field label="Description"><Input bind:value={form.description} placeholder="My application" /></Field>
		<Field
			label="Command (ExecStart)"
			required
			error={submitted ? errors.execStart : null}
			class="col-span-2"
		>
			<Input
				bind:value={form.execStart}
				placeholder="/usr/bin/node /srv/app/server.js"
				class="font-mono text-xs"
				spellcheck="false"
			/>
		</Field>
		<Field label="Run as user" hint="Empty runs as root."
			><Input bind:value={form.user} placeholder="www-data" spellcheck="false" /></Field
		>
		<Field label="Restart policy">
			<SelectField
				bind:value={form.restart}
				options={[
					{ value: 'onFailure', label: 'On failure' },
					{ value: 'always', label: 'Always' },
					{ value: 'no', label: 'Never' }
				]}
			/>
		</Field>
		<Field label="Working directory" class="col-span-2"
			><Input
				bind:value={form.workingDir}
				placeholder="/srv/app"
				class="font-mono text-xs"
				spellcheck="false"
			/></Field
		>
		<Field label="Environment" hint="One KEY=value per line." class="col-span-2">
			<Textarea bind:value={form.environment} rows={3} class="font-mono text-xs" spellcheck="false" />
		</Field>
		<label class="flex items-center gap-2 text-sm"
			><Checkbox bind:checked={form.enable} /> Enable on boot</label
		>
		<label class="flex items-center gap-2 text-sm"><Checkbox bind:checked={form.start} /> Start now</label>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
		<Button type="submit" form="service-form" disabled={creating}
			>{creating ? 'Creating…' : 'Create service'}</Button
		>
	{/snippet}
</Modal>
