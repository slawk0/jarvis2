<!-- Request analytics: totals, traffic over time, world map and top countries. -->
<script lang="ts">
	import Field from '$lib/components/Field.svelte';
	import LineChart from '$lib/components/LineChart.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { Input } from '$lib/components/ui/input';
	import { formatNumber } from '$lib/format';
	import { toIpcError, type IpcError } from '$lib/ipc';
	import { debounce } from '$lib/utils';
	import {
		analyticsFromLogs,
		countryName,
		fetchAnalytics,
		listAll,
		orgPath,
		str,
		type Analytics
	} from './client';
	import WorldMap from './WorldMap.svelte';

	interface Props {
		visible: boolean;
	}

	let { visible }: Props = $props();

	let range = $state('24');
	let resource = $state('');
	let resourceMode = $state<'include' | 'exclude'>('include');
	let action = $state('');
	let location = $state('');
	let host = $state('');
	let resources = $state<{ id: string; name: string }[]>([]);
	let data = $state<(Analytics & { capped?: boolean }) | null>(null);
	let error = $state<IpcError | null>(null);
	let loading = $state(false);
	let seq = 0;

	async function load(): Promise<void> {
		const current = ++seq;
		loading = true;
		try {
			const hours = Number(range);
			const extra = action || location.trim() || host.trim() || (resource && resourceMode === 'exclude');
			let next: Analytics & { capped?: boolean };
			if (extra) {
				const excluded =
					resource && resourceMode === 'exclude'
						? [resources.find((r) => r.id === resource)?.name ?? resource]
						: [];
				next = await analyticsFromLogs(
					hours,
					{
						action: action || undefined,
						location: location.trim().toUpperCase() || undefined,
						host: host.trim() || undefined,
						resourceId: resource && resourceMode === 'include' ? resource : undefined
					},
					excluded
				);
			} else {
				next = await fetchAnalytics(hours, { resourceId: resource || undefined });
			}
			if (current !== seq) return;
			data = next;
			error = null;
		} catch (raw) {
			if (current === seq) error = toIpcError(raw);
		} finally {
			if (current === seq) loading = false;
		}
	}

	const reload = debounce(() => void load(), 400);

	let started = false;
	$effect(() => {
		if (!visible || started) return;
		started = true;
		void load();
		listAll(orgPath('/resources'), ['resources']).then(
			(list) => (resources = list.map((r) => ({ id: str(r.resourceId), name: str(r.name) }))),
			() => {}
		);
	});

	const share = (count: number) => (data?.total ? (count / data.total) * 100 : 0);

	export const refresh = load;
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
	<div class="flex flex-wrap items-end gap-2">
		<Field label="Range">
			<SelectField
				bind:value={range}
				class="w-36"
				onchange={load}
				options={[
					{ value: '24', label: 'Last 24 hours' },
					{ value: '168', label: 'Last 7 days' },
					{ value: '720', label: 'Last 30 days' }
				]}
			/>
		</Field>
		<Field label="Resource">
			<div class="flex gap-1">
				<SelectField
					bind:value={resourceMode}
					class="w-28"
					onchange={() => resource && load()}
					options={[
						{ value: 'include', label: 'Only' },
						{ value: 'exclude', label: 'All except' }
					]}
				/>
				<SelectField
					bind:value={resource}
					class="w-52"
					onchange={load}
					options={[
						{ value: '', label: 'All resources' },
						...resources.map((r) => ({ value: r.id, label: r.name }))
					]}
				/>
			</div>
		</Field>
		<Field label="Action">
			<SelectField
				bind:value={action}
				class="w-32"
				onchange={load}
				options={[
					{ value: '', label: 'Any' },
					{ value: 'true', label: 'Allowed' },
					{ value: 'false', label: 'Blocked' }
				]}
			/>
		</Field>
		<Field label="Country code"
			><Input
				bind:value={location}
				oninput={reload}
				placeholder="e.g. DE"
				class="w-28 uppercase placeholder:normal-case"
				maxlength={2}
			/></Field
		>
		<Field label="Host"
			><Input
				bind:value={host}
				oninput={reload}
				placeholder="app.example.com"
				class="w-56"
				spellcheck="false"
			/></Field
		>
		<span class="flex-1"></span>
		<RefreshControl onrefresh={load} {loading} />
	</div>

	{#if error && !data}
		<StateView kind="error" {error} onretry={load} />
	{:else if !data}
		<StateView kind="loading" />
	{:else}
		{#if error}<p class="selectable text-xs text-destructive" role="alert">{error.message}</p>{/if}
		{#if data.capped}
			<p class="text-xs text-muted-foreground">
				These filters are computed from the newest 5,000 log entries of the range.
			</p>
		{/if}
		<div class="grid grid-cols-2 gap-3 xl:grid-cols-4">
			{#each [{ label: 'Requests', value: formatNumber(data.total), tone: '' }, { label: 'Allowed', value: formatNumber(data.allowed), tone: 'text-success' }, { label: 'Blocked', value: formatNumber(data.blocked), tone: 'text-destructive' }, { label: 'Block rate', value: `${data.blockRate.toFixed(1)}%`, tone: '' }] as card (card.label)}
				<section class="rounded-xl border bg-card p-4">
					<p class="text-xs text-muted-foreground">{card.label}</p>
					<p class="text-2xl font-semibold tabular {card.tone}">{card.value}</p>
				</section>
			{/each}
		</div>

		<section class="rounded-xl border bg-card p-4">
			<h3 class="mb-2 text-sm font-semibold">Traffic over time</h3>
			{#if data.timeline.length > 1}
				<LineChart
					height={180}
					format={(v) => formatNumber(Math.round(v))}
					series={[
						{ label: 'Requests', color: 'var(--primary)', values: data.timeline.map((p) => p.total) },
						{ label: 'Blocked', color: 'var(--destructive)', values: data.timeline.map((p) => p.blocked) }
					]}
					labels={[data.timeline[0].label, data.timeline[data.timeline.length - 1].label]}
				/>
			{:else}
				<p class="text-xs text-muted-foreground">Not enough data for a chart in this range.</p>
			{/if}
		</section>

		<div class="grid grid-cols-1 gap-3 xl:grid-cols-3">
			<section class="rounded-xl border bg-card p-4 xl:col-span-2">
				<h3 class="mb-2 text-sm font-semibold">Requests by country</h3>
				<WorldMap countries={data.countries} />
			</section>
			<section class="rounded-xl border bg-card p-4">
				<h3 class="mb-2 text-sm font-semibold">Top countries</h3>
				{#each data.countries.slice(0, 15) as country (country.code)}
					<div class="relative flex items-center gap-2 border-t py-1 text-sm first:border-t-0">
						<div
							class="absolute inset-y-0.5 left-0 rounded bg-primary/10"
							style="width: {share(country.count)}%"
						></div>
						<span class="relative w-7 font-mono text-xs text-muted-foreground">{country.code}</span>
						<span class="relative min-w-0 flex-1 truncate">{countryName(country.code)}</span>
						{#if country.blocked}<span class="relative text-xs text-destructive tabular"
								>{formatNumber(country.blocked)} blocked</span
							>{/if}
						<span class="relative text-xs text-muted-foreground tabular">{formatNumber(country.count)}</span>
					</div>
				{:else}
					<p class="text-xs text-muted-foreground">No requests in this range.</p>
				{/each}
			</section>
		</div>
	{/if}
</div>
