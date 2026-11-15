<!--
	Request audit log: date range, per-column filters, paging and a detail drawer.
	IP filters are applied here (the API has no exclude filter), over at most
	5000 matching rows that are then paged locally.
-->
<script lang="ts">
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import IpLink from '$lib/components/IpLink.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { Input } from '$lib/components/ui/input';
	import { formatDateTime, formatNumber } from '$lib/format';
	import { toIpcError, type IpcError } from '$lib/ipc';
	import { cn } from '$lib/utils';
	import {
		countryName,
		fetchAllLogs,
		fetchLogs,
		ipMatches,
		listAll,
		orgPath,
		parseIpList,
		str,
		type Json,
		type LogEntry
	} from './client';

	interface Props {
		visible: boolean;
	}

	let { visible }: Props = $props();

	const PAGE = 50;
	const pad = (n: number) => String(n).padStart(2, '0');
	/** Value for a `datetime-local` input, in local time. */
	const localInput = (d: Date) =>
		`${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;

	let from = $state(localInput(new Date(Date.now() - 24 * 3_600_000)));
	let to = $state('');
	let action = $state('');
	let method = $state('');
	let resource = $state('');
	let location = $state('');
	let host = $state('');
	let path = $state('');
	let reason = $state('');
	let actor = $state('');
	let ips = $state('');
	let excludedIps = $state('');
	let resources = $state<{ id: string; name: string }[]>([]);

	let page = $state(1);
	let entries = $state<LogEntry[]>([]);
	let total = $state<number | null>(null);
	let loading = $state(false);
	let error = $state<IpcError | null>(null);
	let detail = $state<LogEntry | null>(null);
	/** All rows kept by the IP filters (paged locally), or null when no IP filter is set. */
	let ipFiltered = $state<LogEntry[] | null>(null);
	let capped = $state(false);
	let seq = 0;

	const pages = $derived(total === null ? null : Math.max(1, Math.ceil(total / PAGE)));
	const hasNext = $derived(pages === null ? entries.length === PAGE : page < pages);

	/** Adds resources to the filter list; the log also names resources the resource list may not return. */
	function addResources(found: { id: string; name: string }[]): void {
		const known = new Set(resources.map((r) => r.id));
		const fresh = found.filter((r) => !known.has(r.id)).map((r) => ({ id: r.id, name: r.name || r.id }));
		if (fresh.length) resources = [...resources, ...fresh].sort((a, b) => a.name.localeCompare(b.name));
	}

	function showLocalPage(): void {
		entries = (ipFiltered ?? []).slice((page - 1) * PAGE, page * PAGE);
		total = ipFiltered?.length ?? 0;
	}

	async function load(): Promise<void> {
		const current = ++seq;
		loading = true;
		try {
			const start = from ? new Date(from) : new Date(Date.now() - 7 * 86_400_000);
			const end = to ? new Date(to) : new Date();
			const filters = {
				action: action || undefined,
				method: method || undefined,
				resourceId: resource || undefined,
				location: location.trim().toUpperCase() || undefined,
				host: host.trim() || undefined,
				path: path.trim() || undefined,
				reason: reason.trim() || undefined,
				actor: actor.trim() || undefined
			};
			const only = parseIpList(ips);
			const excluded = parseIpList(excludedIps);
			if (only.length || excluded.length) {
				const result = await fetchAllLogs(start, end, filters);
				if (current !== seq) return;
				ipFiltered = result.entries.filter(
					(e) => (!only.length || ipMatches(e.ip, only)) && !ipMatches(e.ip, excluded)
				);
				capped = result.capped;
				addResources(result.resources);
				showLocalPage();
			} else {
				const result = await fetchLogs(start, end, filters, PAGE, (page - 1) * PAGE);
				if (current !== seq) return;
				ipFiltered = null;
				capped = false;
				addResources(result.resources);
				entries = result.entries;
				total = result.total;
			}
			error = null;
		} catch (raw) {
			if (current === seq) error = toIpcError(raw);
		} finally {
			if (current === seq) loading = false;
		}
	}

	function apply(): void {
		page = 1;
		void load();
	}

	function clear(): void {
		action = method = resource = location = host = path = reason = actor = ips = excludedIps = '';
		apply();
	}

	function go(next: number): void {
		page = Math.max(1, next);
		if (ipFiltered) showLocalPage();
		else void load();
	}

	function onlyIp(ip: string): void {
		ips = ip;
		apply();
	}

	function excludeIp(ip: string): void {
		const list = parseIpList(excludedIps);
		if (!list.includes(ip)) excludedIps = [...list, ip].join(', ');
		apply();
	}

	let started = false;
	$effect(() => {
		if (!visible || started) return;
		started = true;
		void load();
		listAll(orgPath('/resources'), ['resources']).then(
			(list) => addResources(list.map((r) => ({ id: str(r.resourceId), name: str(r.name) }))),
			() => {}
		);
	});

	const columns: Column<LogEntry>[] = [
		{ key: 'time', label: 'Time', value: (e) => e.time?.getTime() ?? 0, class: 'w-44 tabular' },
		{ key: 'action', label: 'Action', value: (e) => (e.allowed ? 'Allowed' : 'Blocked'), class: 'w-24' },
		{ key: 'ip', label: 'IP', value: (e) => e.ip, class: 'w-40' },
		{ key: 'location', label: 'Location', value: (e) => e.location, class: 'w-24' },
		{ key: 'resource', label: 'Resource', value: (e) => e.resource, class: 'w-40 max-w-40' },
		{ key: 'host', label: 'Host', value: (e) => e.host, class: 'w-52 max-w-52' },
		{ key: 'path', label: 'Path', value: (e) => e.path, mono: true, class: 'max-w-0 w-full' },
		{ key: 'method', label: 'Method', value: (e) => e.method, mono: true, class: 'w-20' },
		{ key: 'reason', label: 'Reason', value: (e) => e.reason, class: 'w-52 max-w-52' },
		{ key: 'actor', label: 'Actor', value: (e) => e.actor, class: 'w-40 max-w-40' }
	];

	/** Group the raw record for the drawer: plain fields, then headers / TLS / other objects. */
	function sections(raw: Json): { title: string; rows: [string, string][] }[] {
		const plain: [string, string][] = [];
		const groups: { title: string; rows: [string, string][] }[] = [];
		for (const [key, value] of Object.entries(raw)) {
			let parsed: unknown = value;
			if (typeof value === 'string' && /^[[{]/.test(value.trim())) {
				try {
					parsed = JSON.parse(value);
				} catch {
					parsed = value;
				}
			}
			if (parsed && typeof parsed === 'object') {
				const rows = Object.entries(parsed as Json).map(([k, v]): [string, string] => [
					k,
					typeof v === 'object' ? JSON.stringify(v) : str(v)
				]);
				if (rows.length) groups.push({ title: key, rows });
			} else if (value !== null && value !== '') {
				plain.push([key, str(value)]);
			}
		}
		return [{ title: 'Request', rows: plain }, ...groups];
	}

	export const refresh = load;
</script>

<div class="flex min-h-0 flex-1 flex-col gap-2">
	<form
		class="flex flex-wrap items-center gap-2"
		onsubmit={(e) => {
			e.preventDefault();
			apply();
		}}
	>
		<label class="flex items-center gap-1.5 text-xs text-muted-foreground"
			>From <Input type="datetime-local" bind:value={from} class="h-8 w-48" /></label
		>
		<label class="flex items-center gap-1.5 text-xs text-muted-foreground"
			>To <Input type="datetime-local" bind:value={to} class="h-8 w-48" /></label
		>
		<SelectField
			bind:value={action}
			size="sm"
			class="w-28"
			onchange={apply}
			options={[
				{ value: '', label: 'Any action' },
				{ value: 'true', label: 'Allowed' },
				{ value: 'false', label: 'Blocked' }
			]}
		/>
		<SelectField
			bind:value={method}
			size="sm"
			class="w-28"
			onchange={apply}
			options={[{ value: '', label: 'Any method' }, 'GET', 'POST', 'PUT', 'PATCH', 'DELETE']}
		/>
		<SelectField
			bind:value={resource}
			size="sm"
			class="w-44"
			onchange={apply}
			options={[
				{ value: '', label: 'Any resource' },
				...resources.map((r) => ({ value: r.id, label: r.name }))
			]}
		/>
		<Input
			bind:value={location}
			placeholder="Country"
			class="h-8 w-20 uppercase placeholder:normal-case"
			maxlength={2}
		/>
		<Input bind:value={host} placeholder="Host" class="h-8 w-40" spellcheck="false" />
		<Input bind:value={path} placeholder="Path" class="h-8 w-36" spellcheck="false" />
		<Input bind:value={reason} placeholder="Reason code" class="h-8 w-28" />
		<Input bind:value={actor} placeholder="Actor" class="h-8 w-32" spellcheck="false" />
		<Input
			bind:value={ips}
			placeholder="IP"
			title="Show only these IPs (comma separated; 10.0.* matches a prefix)"
			class="h-8 w-36 font-mono text-xs placeholder:font-sans"
			spellcheck="false"
		/>
		<Input
			bind:value={excludedIps}
			placeholder="Exclude IPs"
			title="Hide these IPs (comma separated; 10.0.* matches a prefix)"
			class="h-8 w-44 font-mono text-xs placeholder:font-sans"
			spellcheck="false"
		/>
		<Button type="submit" size="sm">Apply</Button>
		<Button type="button" variant="ghost" size="sm" onclick={clear}>Clear</Button>
		<span class="flex-1"></span>
		<RefreshControl onrefresh={load} {loading} />
	</form>

	<DataTable
		rows={entries}
		{columns}
		rowKey={(e) => e.id}
		{loading}
		{error}
		onretry={load}
		empty="No requests match"
		emptyHint="Widen the date range or clear the filters."
		onrowclick={(e) => (detail = e)}
		class="flex-1"
	>
		{#snippet menu(e)}
			<ContextMenu.Item onclick={() => (detail = e)}>Details</ContextMenu.Item>
			<ContextMenu.Separator />
			<ContextMenu.Item disabled={!e.ip} onclick={() => onlyIp(e.ip)}>Show only {e.ip}</ContextMenu.Item>
			<ContextMenu.Item disabled={!e.ip} onclick={() => excludeIp(e.ip)}>Exclude {e.ip}</ContextMenu.Item>
		{/snippet}
		{#snippet cell(e, column)}
			{#if column.key === 'time'}{e.time ? formatDateTime(e.time) : ''}
			{:else if column.key === 'action'}
				<span
					class={cn(
						'rounded px-1.5 py-0.5 text-[11px] font-medium',
						e.allowed ? 'bg-success/15 text-success' : 'bg-destructive/15 text-destructive'
					)}>{e.allowed ? 'Allowed' : 'Blocked'}</span
				>
			{:else if column.key === 'ip'}<IpLink ip={e.ip} class="text-xs" />
			{:else if column.key === 'location'}<span title={e.location ? countryName(e.location) : undefined}
					>{e.location}</span
				>
			{:else}<span title={String(column.value?.(e) ?? '')}>{column.value?.(e)}</span>{/if}
		{/snippet}
	</DataTable>

	<div class="flex items-center gap-2 text-xs text-muted-foreground">
		<span class="tabular"
			>{total === null
				? `${entries.length} shown`
				: `${formatNumber(total)} request${total === 1 ? '' : 's'}`}</span
		>
		{#if capped}
			<span class="text-warning" title="IP filters are applied to the newest 5000 matching requests"
				>IP filters searched only the newest 5000 requests; narrow the date range for more</span
			>
		{/if}
		<span class="flex-1"></span>
		<IconButton label="Previous page" disabled={page <= 1} onclick={() => go(page - 1)}
			><ChevronLeft /></IconButton
		>
		<span class="tabular">Page {page}{pages ? ` of ${formatNumber(pages)}` : ''}</span>
		<IconButton label="Next page" disabled={!hasNext} onclick={() => go(page + 1)}
			><ChevronRight /></IconButton
		>
	</div>
</div>

{#if detail}
	{@const shown = detail}
	<Modal
		open
		title="Request details"
		description="{shown.method} {shown.host}{shown.path}"
		size="xl"
		onclose={() => (detail = null)}
	>
		<div class="flex flex-col gap-4">
			{#each sections(shown.raw) as section (section.title)}
				<section>
					<h3 class="mb-1 text-sm font-semibold capitalize">{section.title}</h3>
					<dl class="grid grid-cols-[12rem_1fr] rounded-lg border bg-card text-sm">
						{#each section.rows as [key, value] (key)}
							<dt class="truncate border-b border-border/50 px-3 py-1 text-muted-foreground" title={key}>
								{key}
							</dt>
							<dd class="selectable border-b border-border/50 px-3 py-1 font-mono text-xs break-all">
								{#if key === 'ip'}<IpLink
										ip={value}
									/>{:else if key === 'reason'}{shown.reason}{:else}{value}{/if}
							</dd>
						{/each}
					</dl>
				</section>
			{/each}
		</div>
	</Modal>
{/if}
