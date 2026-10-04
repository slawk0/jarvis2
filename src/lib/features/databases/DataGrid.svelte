<!-- The Data tab: a paged, server-sorted, filterable grid of one table with row editing. -->
<script lang="ts">
	import { save as saveDialog } from '@tauri-apps/plugin-dialog';
	import ArrowDown from '@lucide/svelte/icons/arrow-down';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Download from '@lucide/svelte/icons/download';
	import FilterIcon from '@lucide/svelte/icons/list-filter';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import X from '@lucide/svelte/icons/x';
	import IconButton from '$lib/components/IconButton.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Input } from '$lib/components/ui/input';
	import { formatNumber } from '$lib/format';
	import {
		api,
		toIpcError,
		type DataPage,
		type ExportFormat,
		type Filter,
		type IpcError,
		type Sort,
		type TableRef
	} from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { cn } from '$lib/utils';
	import RowDialog from './RowDialog.svelte';

	interface Props {
		connection: string;
		table: TableRef;
		/** Views cannot be edited. */
		readonly: boolean;
	}

	let { connection, table, readonly }: Props = $props();

	const OPERATORS = ['=', '!=', '<', '>', '<=', '>=', 'LIKE', 'IS NULL', 'IS NOT NULL'];
	const PAGE_SIZES = ['10', '25', '50', '100', '200'];

	let page = $state(1);
	let pageSize = $state('50');
	let sort = $state<Sort | null>(null);
	/** Filters being edited, and the ones last applied. */
	let filters = $state<Filter[]>([]);
	let applied = $state<Filter[]>([]);
	let filtersOpen = $state(false);
	let data = $state<DataPage | null>(null);
	let error = $state<IpcError | null>(null);
	let loading = $state(false);
	let selected = $state(new Set<number>());
	/** `{ index: null }` adds a row; a number edits that row. */
	let editing = $state<{ index: number | null } | null>(null);
	let seq = 0;

	const pages = $derived(data ? Math.max(1, Math.ceil(data.total / Number(pageSize))) : 1);
	const hasKey = $derived((data?.primaryKey.length ?? 0) > 0);
	const tableKey = $derived(`${table.database}\n${table.schema}\n${table.table}`);

	async function load(): Promise<void> {
		const current = ++seq;
		loading = true;
		try {
			const result = await api.dbTableData(connection, {
				table: $state.snapshot(table),
				page,
				pageSize: Number(pageSize),
				sort: $state.snapshot(sort),
				filters: $state.snapshot(applied)
			});
			if (current !== seq) return;
			data = result;
			error = null;
			selected = new Set();
		} catch (raw) {
			if (current !== seq) return;
			error = toIpcError(raw);
		} finally {
			if (current === seq) loading = false;
		}
	}

	// A different table starts from a clean slate.
	let shownKey = '';
	$effect(() => {
		if (tableKey === shownKey) return;
		shownKey = tableKey;
		page = 1;
		sort = null;
		filters = [];
		applied = [];
		data = null;
		void load();
	});

	function toggleSort(column: string): void {
		if (sort?.column !== column) sort = { column, descending: false };
		else if (!sort.descending) sort = { column, descending: true };
		else sort = null;
		page = 1;
		void load();
	}

	function go(next: number): void {
		page = Math.min(Math.max(1, next), pages);
		void load();
	}

	function applyFilters(): void {
		applied = $state.snapshot(filters).filter((f) => f.column);
		page = 1;
		void load();
	}

	function addFilter(): void {
		filters.push({ column: data?.columns[0]?.name ?? '', operator: '=', value: '' });
		filtersOpen = true;
	}

	function removeFilter(index: number): void {
		filters.splice(index, 1);
		applyFilters();
	}

	/** Identify a row: by primary key, or by every column when there is none. */
	function keyOf(row: (string | null)[]): [string, string | null][] {
		if (!data) return [];
		const names = hasKey ? data.primaryKey : data.columns.map((c) => c.name);
		return names.map((name) => [name, row[data!.columns.findIndex((c) => c.name === name)] ?? null]);
	}

	function toggle(index: number): void {
		const next = new Set(selected);
		if (next.has(index)) next.delete(index);
		else next.add(index);
		selected = next;
	}

	function toggleAll(): void {
		selected = selected.size === data?.rows.length ? new Set() : new Set(data?.rows.map((_, i) => i));
	}

	async function removeSelected(): Promise<void> {
		if (!data || selected.size === 0) return;
		const count = selected.size;
		const ok = await confirm({
			title: `Delete ${count} row${count === 1 ? '' : 's'} from ${table.table}?`,
			message: hasKey
				? 'The rows are deleted in one transaction. This cannot be undone.'
				: 'This table has no primary key: every row identical to a selected one is deleted too. This cannot be undone.',
			confirmLabel: 'Delete',
			destructive: true
		});
		if (!ok) return;
		try {
			const keys = [...selected].map((i) => keyOf(data!.rows[i]));
			const deleted = await api.dbRowsDelete(connection, $state.snapshot(table), keys);
			toast.success(`${deleted} row${deleted === 1 ? '' : 's'} deleted`);
			await load();
		} catch (e) {
			toast.error(e, 'Could not delete the rows');
		}
	}

	async function exportTable(format: ExportFormat): Promise<void> {
		const path = await saveDialog({
			defaultPath: `${table.table}.${format}`,
			filters: [{ name: format.toUpperCase(), extensions: [format] }]
		});
		if (!path) return;
		try {
			const rows = await api.dbExportTable(
				connection,
				$state.snapshot(table),
				$state.snapshot(applied),
				format,
				path
			);
			toast.success(`${formatNumber(rows)} rows exported`);
		} catch (e) {
			toast.error(e, 'Export failed');
		}
	}

	export const refresh = load;
</script>

<div class="flex min-h-0 flex-1 flex-col gap-2">
	<div class="flex flex-wrap items-center gap-2">
		<Button variant="outline" size="sm" onclick={addFilter} disabled={!data}
			><FilterIcon /> Filter{applied.length ? ` (${applied.length})` : ''}</Button
		>
		{#if !readonly}
			<Button variant="outline" size="sm" disabled={!data} onclick={() => (editing = { index: null })}
				><Plus /> Add row</Button
			>
			<Button variant="outline" size="sm" disabled={selected.size === 0} onclick={removeSelected}
				><Trash2 /> Delete{selected.size ? ` (${selected.size})` : ''}</Button
			>
		{:else}
			<span class="text-xs text-muted-foreground">Views are read-only.</span>
		{/if}
		<span class="flex-1"></span>
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<Button {...props} variant="outline" size="sm" disabled={!data}><Download /> Export</Button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="end">
				<DropdownMenu.Label>Whole table{applied.length ? ' (filtered)' : ''}</DropdownMenu.Label>
				<DropdownMenu.Item onclick={() => exportTable('csv')}>CSV…</DropdownMenu.Item>
				<DropdownMenu.Item onclick={() => exportTable('json')}>JSON…</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
		<RefreshControl onrefresh={load} {loading} />
	</div>

	{#if filtersOpen || filters.length}
		<form
			class="flex flex-col gap-1.5 rounded-lg border bg-card p-2"
			onsubmit={(e) => {
				e.preventDefault();
				applyFilters();
			}}
		>
			{#each filters as filter, index (index)}
				<div class="flex items-center gap-2">
					<SelectField
						bind:value={filter.column}
						options={data?.columns.map((c) => c.name) ?? []}
						class="w-56"
						size="sm"
					/>
					<SelectField bind:value={filter.operator} options={OPERATORS} class="w-32" size="sm" />
					{#if !filter.operator.startsWith('IS')}
						<Input
							bind:value={filter.value}
							class="h-8 flex-1 font-mono text-xs"
							placeholder={filter.operator === 'LIKE' ? '%text%' : 'value'}
							spellcheck="false"
						/>
					{:else}
						<span class="flex-1"></span>
					{/if}
					<IconButton label="Remove filter" onclick={() => removeFilter(index)}><X /></IconButton>
				</div>
			{/each}
			<div class="flex items-center gap-2">
				<Button type="button" variant="ghost" size="xs" onclick={addFilter}><Plus /> Add condition</Button>
				<span class="flex-1"></span>
				<Button
					type="button"
					variant="ghost"
					size="xs"
					onclick={() => {
						filters = [];
						filtersOpen = false;
						applyFilters();
					}}>Clear</Button
				>
				<Button type="submit" size="xs">Apply</Button>
			</div>
		</form>
	{/if}

	{#if data && !hasKey && !readonly}
		<p class="flex items-center gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-1.5 text-xs">
			<TriangleAlert class="size-3.5 shrink-0 text-warning" />
			This table has no primary key. Rows are matched on all of their columns, so editing or deleting one also affects
			identical rows.
		</p>
	{/if}

	{#if error && !data}
		<StateView kind="error" {error} onretry={load} />
	{:else if !data}
		<StateView kind="loading" />
	{:else}
		{#if error}<p class="selectable text-xs text-destructive" role="alert">{error.message}</p>{/if}
		<div class={cn('min-h-0 flex-1 overflow-auto rounded-lg border bg-card', loading && 'opacity-70')}>
			<table class="w-max min-w-full border-separate border-spacing-0 text-sm">
				<thead class="sticky top-0 z-10">
					<tr>
						{#if !readonly}
							<th class="border-b bg-card px-3" style="width: 2.25rem">
								<Checkbox
									checked={data.rows.length > 0 && selected.size === data.rows.length}
									indeterminate={selected.size > 0 && selected.size < data.rows.length}
									onCheckedChange={toggleAll}
									aria-label="Select all"
								/>
							</th>
							<th class="border-b bg-card" style="width: 2rem"></th>
						{/if}
						{#each data.columns as column (column.name)}
							<th
								class="h-8 border-b bg-card px-3 text-left text-xs font-medium whitespace-nowrap text-muted-foreground"
								aria-sort={sort?.column === column.name
									? sort.descending
										? 'descending'
										: 'ascending'
									: undefined}
							>
								<button
									type="button"
									class="inline-flex items-center gap-1 hover:text-foreground"
									title={column.dataType}
									onclick={() => toggleSort(column.name)}
								>
									{column.name}
									{#if column.key === 'PRI'}<span class="text-[10px] text-primary">PK</span>{/if}
									{#if sort?.column === column.name}
										{#if sort.descending}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}
									{/if}
								</button>
							</th>
						{/each}
					</tr>
				</thead>
				<tbody>
					{#each data.rows as row, index (index)}
						<tr
							class={cn('h-8 hover:bg-muted/40', selected.has(index) && 'bg-primary/8')}
							ondblclick={() => !readonly && (editing = { index })}
						>
							{#if !readonly}
								<td class="border-b border-border/50 px-3">
									<Checkbox
										checked={selected.has(index)}
										onCheckedChange={() => toggle(index)}
										aria-label="Select row"
									/>
								</td>
								<td class="border-b border-border/50">
									<IconButton label="Edit row" onclick={() => (editing = { index })}><Pencil /></IconButton>
								</td>
							{/if}
							{#each row as value, i (i)}
								<td
									class="selectable max-w-96 truncate border-b border-border/50 px-3 font-mono text-xs"
									title={value !== null && value.length > 40 ? value.slice(0, 2000) : undefined}
								>
									{#if value === null}<span class="text-muted-foreground/70 italic">NULL</span
										>{:else}{value}{/if}
								</td>
							{/each}
						</tr>
					{:else}
						<tr
							><td
								colspan={data.columns.length + 2}
								class="px-3 py-8 text-center text-sm text-muted-foreground"
								>{applied.length ? 'No rows match the filter' : 'This table is empty'}</td
							></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
		<div class="flex items-center gap-2 text-xs text-muted-foreground">
			<span class="tabular">{formatNumber(data.total)} row{data.total === 1 ? '' : 's'}</span>
			<span class="flex-1"></span>
			<span>Rows per page</span>
			<SelectField bind:value={pageSize} options={PAGE_SIZES} size="sm" class="w-20" onchange={() => go(1)} />
			<IconButton label="Previous page" disabled={page <= 1} onclick={() => go(page - 1)}
				><ChevronLeft /></IconButton
			>
			<span class="tabular">Page {page} of {formatNumber(pages)}</span>
			<IconButton label="Next page" disabled={page >= pages} onclick={() => go(page + 1)}
				><ChevronRight /></IconButton
			>
		</div>
	{/if}
</div>

{#if editing && data}
	<RowDialog
		{connection}
		{table}
		columns={data.columns}
		row={editing.index === null ? null : data.rows[editing.index]}
		rowKey={editing.index === null ? [] : keyOf(data.rows[editing.index])}
		onclose={(changed) => {
			editing = null;
			if (changed) void load();
		}}
	/>
{/if}
