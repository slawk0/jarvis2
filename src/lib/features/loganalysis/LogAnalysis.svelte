<script lang="ts">
	import Play from '@lucide/svelte/icons/play';
	import Field from '$lib/components/Field.svelte';
	import IpLink from '$lib/components/IpLink.svelte';
	import LineChart from '$lib/components/LineChart.svelte';
	import Page from '$lib/components/Page.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import TargetProfiles, { execTarget } from '$lib/components/TargetProfiles.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { formatNumber } from '$lib/format';
	import { api, toIpcError, type Count, type IpcError, type LogAnalysis } from '$lib/ipc';
	import { loadDoc, saveDoc, type LogAnalysisProfile } from '$lib/services/profile-data';
	import { resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';

	let { visible }: TabProps = $props();

	const DEFAULT_PATH: Record<LogAnalysisProfile['server'], string> = {
		nginx: '/var/log/nginx/access.log',
		apache: '/var/log/apache2/access.log',
		httpd: '/var/log/httpd/access_log',
		traefik: '/var/log/traefik/access.log'
	};

	const SERVERS: { value: LogAnalysisProfile['server']; label: string }[] = [
		{ value: 'nginx', label: 'Nginx' },
		{ value: 'apache', label: 'Apache (Debian/Ubuntu)' },
		{ value: 'httpd', label: 'Apache httpd (RHEL)' },
		{ value: 'traefik', label: 'Traefik' }
	];

	const blank = (): LogAnalysisProfile => ({
		id: '',
		name: 'Web server',
		kind: 'host',
		container: '',
		server: 'nginx',
		logPath: DEFAULT_PATH.nginx
	});
	const profiles = resource(() => loadDoc('logAnalysisProfiles', []));
	let selectedId = $state('');
	let lines = $state('50000');
	let result = $state<LogAnalysis | null>(null);
	let error = $state<IpcError | null>(null);
	let running = $state(false);

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void profiles.refresh();
		}
	});
	$effect(() => {
		const list = profiles.data;
		if (list?.length && !list.some((p) => p.id === selectedId)) selectedId = list[0].id;
	});
	const selected = $derived(profiles.data?.find((p) => p.id === selectedId));

	async function save(next: LogAnalysisProfile[]): Promise<void> {
		await saveDoc('logAnalysisProfiles', next);
		profiles.set(next);
	}

	async function analyze(): Promise<void> {
		if (!selected) return;
		running = true;
		error = null;
		try {
			result = await api.logAnalyze(
				{ target: execTarget(selected), logPath: selected.logPath },
				Number(lines)
			);
		} catch (raw) {
			error = toIpcError(raw);
		} finally {
			running = false;
		}
	}

	const statusTone = (code: string) =>
		code.startsWith('2')
			? 'bg-success'
			: code.startsWith('3')
				? 'bg-info'
				: code.startsWith('4')
					? 'bg-warning'
					: code.startsWith('5')
						? 'bg-destructive'
						: 'bg-muted-foreground';
	const share = (count: number) => (result?.total ? (count / result.total) * 100 : 0);

	export const refresh = analyze;
</script>

{#snippet ranking(title: string, rows: Count[], kind: 'ip' | 'mono' | 'text')}
	<section class="rounded-xl border bg-card p-4">
		<h3 class="mb-2 text-sm font-semibold">{title}</h3>
		{#each rows.slice(0, 12) as row (row.key)}
			<div class="relative flex items-center gap-2 border-t py-1 text-sm first:border-t-0">
				<div
					class="absolute inset-y-0.5 left-0 rounded bg-primary/10"
					style="width: {share(row.count)}%"
				></div>
				{#if kind === 'ip'}
					<IpLink ip={row.key} class="relative min-w-0 flex-1 truncate text-xs" />
				{:else}
					<span
						class={cn('selectable relative min-w-0 flex-1 truncate', kind === 'mono' && 'font-mono text-xs')}
						title={row.key}>{row.key}</span
					>
				{/if}
				<span class="relative text-xs text-muted-foreground tabular">{formatNumber(row.count)}</span>
			</div>
		{:else}
			<p class="text-xs text-muted-foreground">No data.</p>
		{/each}
	</section>
{/snippet}

{#snippet picker()}
	<TargetProfiles
		noun="Analysis profile"
		description="Choose the web server whose access log you want to analyse, and where it runs."
		profiles={profiles.data ?? []}
		bind:selectedId
		{blank}
		{save}
		validate={(p) =>
			p.logPath && !p.logPath.startsWith('/')
				? 'The log path must be absolute.'
				: !p.logPath && p.kind === 'host'
					? 'Enter the path of the access log.'
					: null}
	>
		{#snippet fields(p)}
			<Field label="Web server">
				<SelectField
					bind:value={p.server}
					options={SERVERS}
					onchange={(server) => (p.logPath = DEFAULT_PATH[server])}
				/>
			</Field>
			<Field
				label="Access log path"
				hint={p.kind === 'container'
					? 'Leave empty to read the container’s output (docker logs).'
					: undefined}
			>
				<Input bind:value={p.logPath} class="font-mono text-xs" spellcheck="false" />
			</Field>
		{/snippet}
	</TargetProfiles>
{/snippet}

{#if profiles.error && !profiles.loaded}
	<StateView kind="error" error={profiles.error} onretry={profiles.refresh} />
{:else if !profiles.loaded}
	<StateView kind="loading" />
{:else if !selected}
	{@render picker()}
{:else}
	<Page>
		{#snippet toolbar()}
			{@render picker()}
			<SelectField
				bind:value={lines}
				class="w-44"
				options={[
					{ value: '1000', label: 'Last 1,000 lines' },
					{ value: '10000', label: 'Last 10,000 lines' },
					{ value: '50000', label: 'Last 50,000 lines' },
					{ value: '200000', label: 'Last 200,000 lines' },
					{ value: '1000000', label: 'Last 1,000,000 lines' }
				]}
			/>
			<Button size="sm" disabled={running} onclick={analyze}
				><Play /> {running ? 'Analysing…' : 'Analyse'}</Button
			>
			<span class="selectable truncate font-mono text-xs text-muted-foreground"
				>{selected.logPath || `docker logs ${selected.container}`}</span
			>
		{/snippet}

		{#if error}
			<StateView
				kind="error"
				{error}
				title={error.code === 'NOT_FOUND' ? 'The access log was not found' : undefined}
				onretry={analyze}
			/>
		{:else if running && !result}
			<StateView kind="loading" title="Reading the log…" />
		{:else if !result}
			<StateView
				kind="empty"
				title="Nothing analysed yet"
				message="Press Analyse to summarise the most recent requests."
			/>
		{:else if result.total === 0}
			<StateView
				kind="empty"
				title="No requests found"
				message="The log is empty or not in the common/combined access-log format."
			/>
		{:else}
			<div class="grid grid-cols-1 gap-3 xl:grid-cols-3">
				<section class="rounded-xl border bg-card p-4">
					<p class="text-xs text-muted-foreground">Total requests</p>
					<p class="text-3xl font-semibold tabular">{formatNumber(result.total)}</p>
					<h3 class="mt-4 mb-2 text-sm font-semibold">Status codes</h3>
					<div class="flex h-3 overflow-hidden rounded-full bg-muted">
						{#each result.statuses as s (s.key)}
							<div
								class={statusTone(s.key)}
								style="width: {share(s.count)}%"
								title="{s.key}: {s.count}"
							></div>
						{/each}
					</div>
					<ul class="mt-2 grid grid-cols-2 gap-x-4 gap-y-0.5 text-sm">
						{#each result.statuses as s (s.key)}
							<li class="flex items-center gap-2">
								<span class={cn('size-2 rounded-full', statusTone(s.key))}></span>
								<span class="font-mono text-xs">{s.key}</span>
								<span class="ml-auto text-xs text-muted-foreground tabular"
									>{formatNumber(s.count)} · {share(s.count).toFixed(1)}%</span
								>
							</li>
						{/each}
					</ul>
				</section>
				<section class="rounded-xl border bg-card p-4 xl:col-span-2">
					<h3 class="mb-2 text-sm font-semibold">Requests per hour</h3>
					<LineChart
						height={170}
						format={(v) => formatNumber(Math.round(v))}
						series={[
							{ label: 'Requests', color: 'var(--primary)', values: result.perHour.map((h) => h.count) }
						]}
						labels={result.perHour.length > 1
							? [result.perHour[0].key, result.perHour[result.perHour.length - 1].key]
							: undefined}
					/>
				</section>
			</div>
			<div class="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
				{@render ranking('Top IP addresses', result.topIps, 'ip')}
				{@render ranking('Top paths', result.topPaths, 'mono')}
				{@render ranking('HTTP methods', result.methods, 'mono')}
				{@render ranking('Top user agents', result.topAgents, 'text')}
			</div>
		{/if}
	</Page>
{/if}
