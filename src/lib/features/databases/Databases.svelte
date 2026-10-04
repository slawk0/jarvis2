<!--
	Databases: saved connections per server, a navigator (databases → schemas →
	tables/views) and, for the selected table, data / structure / SQL.
-->
<script lang="ts">
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import DatabaseIcon from '@lucide/svelte/icons/database';
	import Eye from '@lucide/svelte/icons/eye';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plug from '@lucide/svelte/icons/plug';
	import Plus from '@lucide/svelte/icons/plus';
	import Table2 from '@lucide/svelte/icons/table-2';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Unplug from '@lucide/svelte/icons/unplug';
	import IconButton from '$lib/components/IconButton.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		api,
		toIpcError,
		type DbProfile,
		type IpcError,
		type TableInfo,
		type TableRef,
		type TableStructure
	} from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { resource } from '$lib/state/resource.svelte';
	import { cn, matches } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';
	import DataGrid from './DataGrid.svelte';
	import ProfileDialog from './ProfileDialog.svelte';
	import SqlEditor from './SqlEditor.svelte';

	let { visible }: TabProps = $props();

	const blank = (): DbProfile => ({
		id: '',
		name: '',
		engine: 'mysql',
		source: 'host',
		host: '127.0.0.1',
		port: 3306,
		container: '',
		user: '',
		database: ''
	});

	const profiles = resource((io) => io.dbProfiles());
	let selectedId = $state('');
	let connected = $state(new Set<string>());
	let connecting = $state(false);
	let connectError = $state<IpcError | null>(null);
	let editing = $state<DbProfile | null>(null);

	// Navigator state of the connected profile.
	let databases = $state<string[]>([]);
	let database = $state('');
	let schemas = $state<string[]>([]);
	let schema = $state('');
	let tables = $state<TableInfo[]>([]);
	let tablesLoading = $state(false);
	let navError = $state<IpcError | null>(null);
	let tableFilter = $state('');
	let current = $state<TableInfo | null>(null);
	let view = $state<'data' | 'structure' | 'sql'>('data');
	let structure = $state<TableStructure | null>(null);
	let structureError = $state<IpcError | null>(null);
	let grid = $state<DataGrid | null>(null);

	const selected = $derived(profiles.data?.find((p) => p.id === selectedId));
	const isConnected = $derived(connected.has(selectedId));
	const tableRef = $derived<TableRef | null>(current ? { database, schema, table: current.name } : null);
	const shownTables = $derived(tables.filter((t) => matches(tableFilter, t.name)));

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void profiles.refresh();
			api.dbConnected().then(
				(ids) => (connected = new Set(ids)),
				() => {}
			);
		}
	});
	$effect(() => {
		const list = profiles.data;
		if (list?.length && !list.some((p) => p.id === selectedId)) selectedId = list[0].id;
	});

	// Switching profile (or its connection state) reloads the navigator.
	let navFor = '';
	$effect(() => {
		const key = isConnected ? selectedId : '';
		if (key === navFor) return;
		navFor = key;
		databases = [];
		tables = [];
		schemas = [];
		current = null;
		database = '';
		schema = '';
		navError = null;
		if (key) void loadDatabases();
	});

	async function connect(): Promise<void> {
		if (!selected) return;
		connecting = true;
		connectError = null;
		try {
			await api.dbConnect(selected.id);
			connected = new Set([...connected, selected.id]);
		} catch (raw) {
			connectError = toIpcError(raw);
		} finally {
			connecting = false;
		}
	}

	async function disconnect(): Promise<void> {
		const id = selectedId;
		try {
			await api.dbDisconnect(id);
		} catch (e) {
			toast.error(e);
		}
		const next = new Set(connected);
		next.delete(id);
		connected = next;
	}

	async function loadDatabases(): Promise<void> {
		const id = selectedId;
		try {
			const list = await api.dbDatabases(id);
			if (id !== selectedId) return;
			databases = list;
			const preferred = selected?.database;
			await chooseDatabase(preferred && list.includes(preferred) ? preferred : (list[0] ?? ''));
		} catch (raw) {
			navError = toIpcError(raw);
			if (navError.is('DB_NOT_CONNECTED')) void disconnect();
		}
	}

	async function chooseDatabase(name: string): Promise<void> {
		database = name;
		current = null;
		tables = [];
		schemas = [];
		schema = '';
		if (!name) return;
		try {
			if (selected?.engine === 'postgres') {
				schemas = await api.dbSchemas(selectedId, name);
				schema = schemas.includes('public') ? 'public' : (schemas[0] ?? '');
			}
			await loadTables();
		} catch (raw) {
			navError = toIpcError(raw);
		}
	}

	async function loadTables(): Promise<void> {
		tablesLoading = true;
		navError = null;
		try {
			tables = await api.dbTables(selectedId, database, schema);
			if (current && !tables.some((t) => t.name === current!.name)) current = null;
		} catch (raw) {
			navError = toIpcError(raw);
		} finally {
			tablesLoading = false;
		}
	}

	function openTable(table: TableInfo): void {
		current = table;
		structure = null;
		if (view === 'sql') view = 'data';
	}

	// Structure is loaded when its tab is shown.
	$effect(() => {
		const ref = tableRef;
		if (view !== 'structure' || !ref || !isConnected) return;
		structure = null;
		structureError = null;
		api.dbTableStructure(selectedId, ref).then(
			(s) => (structure = s),
			(raw) => (structureError = toIpcError(raw))
		);
	});

	async function removeProfile(): Promise<void> {
		if (!selected) return;
		const ok = await confirm({
			title: `Remove the connection “${selected.name}”?`,
			message: 'Only the saved connection and its password are removed. The database itself is not touched.',
			confirmLabel: 'Remove',
			destructive: true
		});
		if (!ok) return;
		try {
			await api.dbProfileDelete(selected.id);
			await profiles.refresh();
		} catch (e) {
			toast.error(e);
		}
	}

	async function saved(profile: DbProfile): Promise<void> {
		// A changed profile needs a fresh connection.
		if (connected.has(profile.id)) {
			selectedId = profile.id;
			await disconnect();
		}
		await profiles.refresh();
		selectedId = profile.id;
	}

	export function refresh(): void {
		if (!isConnected) void profiles.refresh();
		else if (current && view === 'data') void grid?.refresh();
		else void loadTables();
	}
</script>

{#if profiles.error && !profiles.loaded}
	<StateView kind="error" error={profiles.error} onretry={profiles.refresh} />
{:else if !profiles.loaded}
	<StateView kind="loading" />
{:else if !profiles.data?.length}
	<StateView
		kind="empty"
		icon={DatabaseIcon}
		title="No database connections yet"
		message="Add a MySQL, MariaDB or PostgreSQL database that this server can reach. Nothing needs to be installed on the server."
	>
		<Button onclick={() => (editing = blank())}><Plus /> Add a connection</Button>
	</StateView>
{:else}
	<div class="flex h-full min-h-0 flex-col">
		<div class="flex min-h-11 shrink-0 flex-wrap items-center gap-2 px-4 py-2">
			<SelectField
				bind:value={selectedId}
				options={profiles.data.map((p) => ({ value: p.id, label: p.name }))}
				class="w-64"
			/>
			{#if selected}
				<span class="text-xs text-muted-foreground">
					{selected.engine === 'mysql' ? 'MySQL / MariaDB' : 'PostgreSQL'} ·
					<span class="font-mono"
						>{selected.user}@{selected.source === 'container'
							? selected.container
							: `${selected.host}:${selected.port}`}</span
					>
				</span>
			{/if}
			<span class="flex-1"></span>
			{#if isConnected}
				<Button variant="outline" size="sm" onclick={disconnect}><Unplug /> Disconnect</Button>
			{:else}
				<Button size="sm" disabled={connecting} onclick={connect}
					><Plug /> {connecting ? 'Connecting…' : 'Connect'}</Button
				>
			{/if}
			<IconButton label="Edit connection" onclick={() => (editing = selected ?? null)}><Pencil /></IconButton>
			<IconButton label="Remove connection" onclick={removeProfile}><Trash2 /></IconButton>
			<Button variant="outline" size="sm" onclick={() => (editing = blank())}><Plus /> New</Button>
		</div>

		{#if !isConnected}
			{#if connectError}
				<StateView
					kind="error"
					error={connectError}
					title="Could not connect to the database"
					onretry={connect}
				/>
			{:else if connecting}
				<StateView kind="loading" title="Connecting…" />
			{:else}
				<StateView kind="empty" icon={Plug} title="Not connected" message="Connect to browse this database.">
					<Button onclick={connect}><Plug /> Connect</Button>
				</StateView>
			{/if}
		{:else}
			<div class="flex min-h-0 flex-1 gap-3 px-4 pb-4">
				<aside class="flex w-64 shrink-0 flex-col gap-2 rounded-lg border bg-card p-2">
					<SelectField
						value={database}
						options={databases}
						placeholder="Database"
						size="sm"
						onchange={chooseDatabase}
					/>
					{#if schemas.length}
						<SelectField
							bind:value={schema}
							options={schemas}
							placeholder="Schema"
							size="sm"
							onchange={loadTables}
						/>
					{/if}
					<SearchInput bind:value={tableFilter} placeholder="Filter tables…" />
					<div class="-mx-1 min-h-0 flex-1 overflow-y-auto">
						{#if navError}
							<p class="selectable p-2 text-xs text-destructive" role="alert">{navError.message}</p>
						{:else if tablesLoading && !tables.length}
							<p class="p-2 text-xs text-muted-foreground">Loading…</p>
						{:else}
							{#each shownTables as table (table.name)}
								<button
									type="button"
									class={cn(
										'flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-sm hover:bg-muted',
										current?.name === table.name && 'bg-primary/12 text-primary'
									)}
									onclick={() => openTable(table)}
								>
									{#if table.view}<Eye class="size-3.5 shrink-0 text-muted-foreground" />{:else}<Table2
											class="size-3.5 shrink-0 text-muted-foreground"
										/>{/if}
									<span class="truncate" title={table.name}>{table.name}</span>
								</button>
							{:else}
								<p class="p-2 text-xs text-muted-foreground">
									{tables.length ? 'No table matches' : 'No tables here'}
								</p>
							{/each}
						{/if}
					</div>
					<p class="px-1 text-[11px] text-muted-foreground">
						{tables.filter((t) => !t.view).length} tables · {tables.filter((t) => t.view).length} views
					</p>
				</aside>

				<div class="flex min-w-0 flex-1 flex-col gap-2">
					<div class="flex items-center gap-3">
						<SubTabs
							bind:value={view}
							class="flex-1"
							items={[
								{ id: 'data', label: 'Data' },
								{ id: 'structure', label: 'Structure' },
								{ id: 'sql', label: 'SQL editor' }
							]}
						/>
						{#if current && view !== 'sql'}
							<span class="flex items-center gap-1 font-mono text-xs text-muted-foreground">
								{database}<ChevronRight class="size-3" />{#if schema}{schema}<ChevronRight
										class="size-3"
									/>{/if}<span class="text-foreground">{current.name}</span>
							</span>
						{/if}
					</div>

					{#if view === 'sql'}
						<SqlEditor connection={selectedId} {database} onran={loadTables} />
					{:else if !tableRef || !current}
						<StateView
							kind="empty"
							icon={ChevronDown}
							title="Choose a table"
							message="Pick a table or view on the left, or write a query in the SQL editor."
						/>
					{:else if view === 'data'}
						<DataGrid bind:this={grid} connection={selectedId} table={tableRef} readonly={current.view} />
					{:else if structureError}
						<StateView kind="error" error={structureError} />
					{:else if !structure}
						<StateView kind="loading" />
					{:else}
						<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto">
							<div class="overflow-hidden rounded-lg border bg-card">
								<table class="w-full text-sm">
									<thead>
										<tr class="border-b text-left text-xs text-muted-foreground">
											<th class="h-8 px-3 font-medium">Column</th>
											<th class="px-3 font-medium">Type</th>
											<th class="px-3 font-medium">Nullable</th>
											<th class="px-3 font-medium">Default</th>
											<th class="px-3 font-medium">Key</th>
											<th class="px-3 font-medium">Extra</th>
										</tr>
									</thead>
									<tbody>
										{#each structure.columns as column (column.name)}
											<tr class="selectable border-b border-border/50 last:border-b-0">
												<td class="h-8 px-3 font-medium">{column.name}</td>
												<td class="px-3 font-mono text-xs">{column.dataType}</td>
												<td class="px-3">{column.nullable ? 'Yes' : 'No'}</td>
												<td class="px-3 font-mono text-xs"
													>{#if column.default === null}<span class="text-muted-foreground/70 italic"
															>none</span
														>{:else}{column.default}{/if}</td
												>
												<td class="px-3 text-xs">{column.key}</td>
												<td class="px-3 text-xs">{column.extra}</td>
											</tr>
										{/each}
									</tbody>
								</table>
							</div>
							{#each [{ title: 'Indexes', rows: structure.indexes }, { title: 'Foreign keys', rows: structure.foreignKeys }] as group (group.title)}
								<section>
									<h3 class="mb-1.5 text-sm font-semibold">{group.title}</h3>
									{#if group.rows.length}
										<div class="rounded-lg border bg-card">
											{#each group.rows as row (row.name)}
												<div
													class="selectable flex gap-4 border-b border-border/50 px-3 py-1.5 text-sm last:border-b-0"
												>
													<span class="w-64 shrink-0 truncate font-medium" title={row.name}>{row.name}</span>
													<span class="font-mono text-xs break-all">{row.definition}</span>
												</div>
											{/each}
										</div>
									{:else}
										<p class="text-xs text-muted-foreground">None.</p>
									{/if}
								</section>
							{/each}
						</div>
					{/if}
				</div>
			</div>
		{/if}
	</div>
{/if}

<ProfileDialog bind:profile={editing} onsaved={saved} />
