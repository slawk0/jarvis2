<!--
	The shared data table: sortable headers, text filter, loading / empty /
	error states, row actions, multi-select with a bulk bar, per-row busy
	state, a shared row context menu, and windowed rendering for long lists.
-->
<script lang="ts" module>
	export interface Column<T> {
		key: string;
		label: string;
		/** Plain value used for sorting, filtering and default rendering. */
		value?: (row: T) => string | number | boolean | null | undefined;
		sortable?: boolean;
		align?: 'left' | 'right' | 'center';
		/** Classes for the cells of this column (e.g. width, font). */
		class?: string;
		mono?: boolean;
	}

	export interface Sort {
		key: string;
		dir: 'asc' | 'desc';
	}
</script>

<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';
	import ArrowDown from '@lucide/svelte/icons/arrow-down';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import type { IpcError } from '$lib/ipc';
	import { cn, compare, matches } from '$lib/utils';
	import StateView from './StateView.svelte';

	interface Props {
		rows: T[];
		columns: Column<T>[];
		rowKey: (row: T) => string;
		/** Custom cell content; falls back to the column's `value`. */
		cell?: Snippet<[T, Column<T>]>;
		/** Right-aligned per-row action buttons. */
		actions?: Snippet<[T]>;
		/** Context-menu items for the row under the pointer. */
		menu?: Snippet<[T]>;
		/** Shown above the table while rows are selected. */
		bulk?: Snippet<[T[]]>;
		/** Filter text matched against every column value. */
		search?: string;
		sort?: Sort | null;
		selectable?: boolean;
		selected?: Set<string>;
		busy?: Set<string>;
		loading?: boolean;
		error?: IpcError | null;
		onretry?: () => void;
		empty?: string;
		emptyHint?: string;
		onrowclick?: (row: T, event: MouseEvent) => void;
		onrowdblclick?: (row: T) => void;
		rowClass?: (row: T) => string | undefined;
		rowHeight?: number;
		class?: string;
	}

	let {
		rows,
		columns,
		rowKey,
		cell,
		actions,
		menu,
		bulk,
		search = '',
		sort = $bindable(null),
		selectable = false,
		selected = $bindable(new Set<string>()),
		busy,
		loading = false,
		error = null,
		onretry,
		empty = 'Nothing to show',
		emptyHint,
		onrowclick,
		onrowdblclick,
		rowClass,
		rowHeight = 33,
		class: className
	}: Props = $props();

	const VIRTUALIZE_FROM = 150;
	const OVERSCAN = 12;

	const filtered = $derived.by(() => {
		const q = search.trim();
		if (!q) return rows;
		return rows.filter((row) => matches(q, ...columns.map((c) => valueText(c, row))));
	});

	const sorted = $derived.by(() => {
		const column = sort && columns.find((c) => c.key === sort!.key);
		if (!sort || !column?.value) return filtered;
		const sign = sort.dir === 'asc' ? 1 : -1;
		const value = column.value;
		return [...filtered].sort((a, b) => sign * compare(value(a), value(b)));
	});

	function valueText(column: Column<T>, row: T): string {
		const v = column.value?.(row);
		return v == null ? '' : String(v);
	}

	function toggleSort(column: Column<T>): void {
		if (!column.value || column.sortable === false) return;
		if (sort?.key !== column.key) sort = { key: column.key, dir: 'asc' };
		else if (sort.dir === 'asc') sort = { key: column.key, dir: 'desc' };
		else sort = null;
	}

	// ---- windowing ----------------------------------------------------------
	let scroller = $state<HTMLDivElement | null>(null);
	let scrollTop = $state(0);
	let viewport = $state(600);
	const virtual = $derived(sorted.length > VIRTUALIZE_FROM);
	const first = $derived(virtual ? Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN) : 0);
	const last = $derived(
		virtual
			? Math.min(sorted.length, Math.ceil((scrollTop + viewport) / rowHeight) + OVERSCAN)
			: sorted.length
	);
	const windowed = $derived(sorted.slice(first, last));
	const colSpan = $derived(columns.length + (selectable ? 1 : 0) + (actions ? 1 : 0));

	$effect(() => {
		if (!scroller) return;
		const observer = new ResizeObserver(() => (viewport = scroller!.clientHeight));
		observer.observe(scroller);
		return () => observer.disconnect();
	});

	// ---- selection ----------------------------------------------------------
	let anchor: string | null = null;
	const selectedRows = $derived(sorted.filter((r) => selected.has(rowKey(r))));
	const allSelected = $derived(sorted.length > 0 && selectedRows.length === sorted.length);

	// Drop selections whose rows disappeared.
	$effect(() => {
		if (selected.size === 0) return;
		const keys = new Set(rows.map(rowKey));
		if ([...selected].some((k) => !keys.has(k))) {
			selected = new Set([...selected].filter((k) => keys.has(k)));
		}
	});

	function toggleAll(): void {
		selected = allSelected ? new Set() : new Set(sorted.map(rowKey));
	}

	function toggleRow(row: T, event?: MouseEvent): void {
		const key = rowKey(row);
		const next = new Set(selected);
		if (event?.shiftKey && anchor) {
			const keys = sorted.map(rowKey);
			const [a, b] = [keys.indexOf(anchor), keys.indexOf(key)].sort((x, y) => x - y);
			if (a >= 0) for (const k of keys.slice(a, b + 1)) next.add(k);
		} else if (next.has(key)) {
			next.delete(key);
		} else {
			next.add(key);
		}
		anchor = key;
		selected = next;
	}

	// ---- context menu -------------------------------------------------------
	let menuRow = $state<T | null>(null);

	const ALIGN = { left: 'text-left', right: 'text-right', center: 'text-center' };
</script>

{#snippet body()}
	<table class="w-full border-separate border-spacing-0 text-sm">
		<thead class="sticky top-0 z-10">
			<tr>
				{#if selectable}
					<th class="bg-card border-b px-3 py-0" style="width: 2.25rem">
						<Checkbox
							checked={allSelected}
							indeterminate={!allSelected && selectedRows.length > 0}
							onCheckedChange={toggleAll}
							aria-label="Select all"
						/>
					</th>
				{/if}
				{#each columns as column (column.key)}
					{@const sortable = column.value !== undefined && column.sortable !== false}
					<th
						class={cn(
							'bg-card text-muted-foreground h-8 border-b px-3 text-xs font-medium whitespace-nowrap',
							ALIGN[column.align ?? 'left'],
							column.class
						)}
						aria-sort={sort?.key === column.key ? (sort.dir === 'asc' ? 'ascending' : 'descending') : undefined}
					>
						{#if sortable}
							<button
								type="button"
								class="hover:text-foreground inline-flex items-center gap-1"
								onclick={() => toggleSort(column)}
							>
								{column.label}
								{#if sort?.key === column.key}
									{#if sort.dir === 'asc'}<ArrowUp class="size-3" />{:else}<ArrowDown class="size-3" />{/if}
								{/if}
							</button>
						{:else}
							{column.label}
						{/if}
					</th>
				{/each}
				{#if actions}
					<th class="bg-card border-b px-3"></th>
				{/if}
			</tr>
		</thead>
		<tbody>
			{#if virtual && first > 0}
				<tr aria-hidden="true"><td colspan={colSpan} style="height: {first * rowHeight}px"></td></tr>
			{/if}
			{#each windowed as row (rowKey(row))}
				{@const key = rowKey(row)}
				{@const isBusy = busy?.has(key) ?? false}
				<tr
					class={cn(
						'group/row hover:bg-muted/40 transition-colors',
						selected.has(key) && 'bg-primary/8 hover:bg-primary/12',
						isBusy && 'opacity-60',
						(onrowclick || onrowdblclick) && 'cursor-pointer',
						rowClass?.(row)
					)}
					style="height: {rowHeight}px"
					aria-busy={isBusy}
					onclick={(e) => onrowclick?.(row, e)}
					ondblclick={() => onrowdblclick?.(row)}
					oncontextmenu={() => (menuRow = row)}
				>
					{#if selectable}
						<td class="border-border/50 border-b px-3">
							<Checkbox
								checked={selected.has(key)}
								onclick={(e: MouseEvent) => {
									e.stopPropagation();
									e.preventDefault();
									toggleRow(row, e);
								}}
								aria-label="Select row"
							/>
						</td>
					{/if}
					{#each columns as column (column.key)}
						<td
							class={cn(
								'border-border/50 selectable truncate border-b px-3',
								ALIGN[column.align ?? 'left'],
								column.mono && 'font-mono text-xs',
								column.class
							)}
						>
							{#if cell}{@render cell(row, column)}{:else}{valueText(column, row)}{/if}
						</td>
					{/each}
					{#if actions}
						<td class="border-border/50 border-b px-2 text-right whitespace-nowrap" style="width: 1%">
							<div class="flex items-center justify-end gap-0.5">
								{#if isBusy}
									<LoaderCircle class="text-muted-foreground mx-1.5 size-4 animate-spin" />
								{:else}
									{@render actions(row)}
								{/if}
							</div>
						</td>
					{/if}
				</tr>
			{/each}
			{#if virtual && last < sorted.length}
				<tr aria-hidden="true">
					<td colspan={colSpan} style="height: {(sorted.length - last) * rowHeight}px"></td>
				</tr>
			{/if}
		</tbody>
	</table>
{/snippet}

<div class={cn('bg-card flex min-h-0 flex-col overflow-hidden rounded-lg border', className)}>
	{#if bulk && selectedRows.length > 0}
		<div class="bg-primary/8 flex items-center gap-2 border-b px-3 py-1.5 text-sm">
			<span class="text-muted-foreground mr-1">{selectedRows.length} selected</span>
			{@render bulk(selectedRows)}
			<button
				type="button"
				class="text-muted-foreground hover:text-foreground ml-auto text-xs"
				onclick={() => (selected = new Set())}
			>
				Clear selection
			</button>
		</div>
	{/if}
	<div
		bind:this={scroller}
		class="min-h-0 flex-1 overflow-auto"
		onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
	>
		{#if error && rows.length === 0}
			<StateView kind="error" {error} {onretry} />
		{:else if loading && rows.length === 0}
			<StateView kind="loading" />
		{:else if sorted.length === 0}
			<StateView
				kind="empty"
				title={rows.length > 0 ? 'No matches' : empty}
				message={rows.length > 0 ? 'Nothing matches the current filter.' : emptyHint}
			/>
		{:else if menu}
			<ContextMenu.Root>
				<ContextMenu.Trigger class="block">{@render body()}</ContextMenu.Trigger>
				<ContextMenu.Content class="w-52">
					{#if menuRow}{@render menu(menuRow)}{/if}
				</ContextMenu.Content>
			</ContextMenu.Root>
		{:else}
			{@render body()}
		{/if}
	</div>
</div>
