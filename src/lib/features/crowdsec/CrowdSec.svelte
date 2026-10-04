<script lang="ts">
	import { parse as parseYaml } from 'yaml';
	import Copy from '@lucide/svelte/icons/copy';
	import Plus from '@lucide/svelte/icons/plus';
	import Settings from '@lucide/svelte/icons/settings';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column, type Sort } from '$lib/components/DataTable.svelte';
	import DependencyGuard from '$lib/components/DependencyGuard.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import IpLink from '$lib/components/IpLink.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { formatDateTime, formatRelative } from '$lib/format';
	import {
		api,
		type Acquisition,
		type Alert,
		type Bouncer,
		type Decision,
		type HubAction,
		type HubItem
	} from '$lib/ipc';
	import { confirm, prompt } from '$lib/services/confirm.svelte';
	import { loadDoc, saveDoc, type CrowdsecConfig } from '$lib/services/profile-data';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { Busy, poll, resource, type Resource } from '$lib/state/resource.svelte';
	import { cn, copyText } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';
	import { workspace } from '$lib/workspace/workspace.svelte';

	let { visible }: TabProps = $props();

	type View = 'dashboard' | 'decisions' | 'whitelist' | 'alerts' | 'bouncers' | 'metrics' | 'hub';
	let view = $state<View>('dashboard');
	let search = $state('');
	const busy = new Busy();

	const status = resource((io) => io.crowdsecStatus());
	const decisions = resource((io) => io.crowdsecDecisions());
	const alerts = resource((io) => io.crowdsecAlerts());
	const bouncers = resource((io) => io.crowdsecBouncers());
	const metrics = resource((io) => io.crowdsecMetrics());
	const hub = resource((io) => io.crowdsecHub());
	const whitelists = resource((io) => io.crowdsecWhitelists());

	const needs: Record<View, Resource<unknown>[]> = {
		dashboard: [decisions, alerts, bouncers, metrics, whitelists],
		decisions: [decisions],
		whitelist: [whitelists],
		alerts: [alerts],
		bouncers: [bouncers],
		metrics: [metrics],
		hub: [hub]
	};

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void status.refresh();
		}
	});
	const ready = $derived(status.data?.installed === true);
	$effect(() => {
		if (!visible || !ready) return;
		for (const r of needs[view]) if (!r.loaded && !r.loading) void r.refresh();
	});
	poll(
		() => visible && ready && view !== 'hub',
		30_000,
		() => Promise.all(needs[view].map((r) => r.load(true)))
	);

	// Other tabs can link straight to a sub-tab.
	$effect(() => {
		if (!visible) return;
		const request = workspace.takeRequest('crowdsec');
		if (request) view = request.view as View;
	});

	async function run(
		key: string,
		reason: string,
		action: () => Promise<unknown>,
		done: string,
		...reload: Resource<unknown>[]
	): Promise<boolean> {
		const ok = await busy.run(
			key,
			() => sudo.describe(reason, action),
			(e) => toast.error(e)
		);
		if (ok && done) toast.success(done);
		await Promise.all(reload.map((r) => r.refresh()));
		return ok;
	}

	// ------------------------------------------------------------ connection settings
	let configOpen = $state(false);
	let config = $state<CrowdsecConfig>({ mode: 'auto', container: '', prefix: '' });

	async function openConfig(): Promise<void> {
		config = await loadDoc('crowdsecConfig', { mode: 'auto', container: '', prefix: '' });
		configOpen = true;
	}

	async function saveConfig(): Promise<void> {
		try {
			await saveDoc('crowdsecConfig', $state.snapshot(config));
			configOpen = false;
			for (const r of [decisions, alerts, bouncers, metrics, hub, whitelists]) r.reset();
			await status.refresh();
			if (status.data?.installed) toast.success(`Connected to CrowdSec ${status.data.version}`);
			else toast.warning('CrowdSec was not found with these settings');
		} catch (error) {
			toast.error(error, 'Connection test failed');
		}
	}

	async function service(action: 'start' | 'stop' | 'restart'): Promise<void> {
		if (status.data?.mode === 'docker') {
			await run(
				'service',
				`${action} CrowdSec`,
				() => api.dockerContainerAction([status.data!.container], action),
				`CrowdSec ${action} done`,
				status
			);
		} else {
			await run(
				'service',
				`${action} CrowdSec`,
				() => api.unitAction('crowdsec.service', action),
				`CrowdSec ${action} done`,
				status
			);
		}
	}

	// ------------------------------------------------------------ dashboard numbers
	function top<T>(items: T[], key: (item: T) => string, limit = 5): [string, number][] {
		const counts = new Map<string, number>();
		for (const item of items) {
			const k = key(item);
			if (k) counts.set(k, (counts.get(k) ?? 0) + 1);
		}
		return [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, limit);
	}
	const parsing = $derived.by(() => {
		const list = metrics.data ?? [];
		const read = list.reduce((s, a) => s + a.read, 0);
		return read ? (list.reduce((s, a) => s + a.parsed, 0) / read) * 100 : null;
	});
	const flag = (code: string) =>
		/^[A-Za-z]{2}$/.test(code)
			? String.fromCodePoint(...[...code.toUpperCase()].map((c) => 127397 + c.charCodeAt(0)))
			: '';

	// ------------------------------------------------------------ decisions
	let decisionSort = $state<Sort | null>(null);
	let banOpen = $state(false);
	let ban = $state({ scope: 'ip', value: '', duration: '4h', custom: '', reason: '' });
	const decisionColumns: Column<Decision>[] = [
		{ key: 'value', label: 'IP / range', value: (d) => d.value, class: 'w-56' },
		{ key: 'kind', label: 'Type', value: (d) => d.kind, class: 'w-24' },
		{ key: 'origin', label: 'Origin', value: (d) => d.origin, class: 'w-32' },
		{ key: 'reason', label: 'Reason', value: (d) => d.reason, class: 'max-w-0 w-full' },
		{
			key: 'country',
			label: 'Country / AS',
			value: (d) => `${d.country} ${d.asName}`,
			class: 'w-64 max-w-64'
		},
		{ key: 'duration', label: 'Remaining', value: (d) => d.duration, class: 'w-32 tabular' },
		{ key: 'until', label: 'Until', value: (d) => d.until, class: 'w-44 tabular' }
	];

	async function addBan(): Promise<void> {
		const duration = ban.duration === 'custom' ? ban.custom : ban.duration;
		if (
			await run(
				'ban',
				'Add a CrowdSec ban',
				() => api.crowdsecBan({ scope: ban.scope, value: ban.value, duration, reason: ban.reason }),
				`${ban.value} banned`,
				decisions
			)
		) {
			banOpen = false;
		}
	}

	async function unbanAll(): Promise<void> {
		const ok = await confirm({
			title: 'Remove all decisions?',
			message: 'Every active ban is lifted, including those made by CrowdSec itself.',
			confirmLabel: 'Unban all',
			destructive: true
		});
		if (ok)
			await run(
				'unban-all',
				'Remove all CrowdSec decisions',
				() => api.crowdsecUnban(null),
				'All decisions removed',
				decisions
			);
	}

	// ------------------------------------------------------------ whitelist
	interface WhitelistEntry {
		value: string;
		source: 'system' | 'jarvis' | 'lapi' | 'custom';
		origin: string;
	}
	const whitelist = $derived.by(() => {
		const entries: WhitelistEntry[] = [];
		for (const file of whitelists.data?.files ?? []) {
			try {
				const doc = parseYaml(file.content) as { whitelist?: { ip?: string[]; cidr?: string[] } } | null;
				const source = file.managed ? 'jarvis' : file.path.endsWith('/whitelists.yaml') ? 'system' : 'custom';
				for (const value of [...(doc?.whitelist?.ip ?? []), ...(doc?.whitelist?.cidr ?? [])]) {
					entries.push({ value: String(value), source, origin: file.path });
				}
			} catch {
				// An unreadable whitelist file is simply not listed.
			}
		}
		for (const list of whitelists.data?.allowlists ?? []) {
			for (const value of list.items) entries.push({ value, source: 'lapi', origin: list.name });
		}
		return entries;
	});
	const SOURCE_LABEL = { system: 'system', jarvis: 'Jarvis', lapi: 'LAPI allowlist', custom: 'custom file' };

	async function saveManaged(values: string[]): Promise<void> {
		const ips = values.filter((v) => !v.includes('/'));
		const cidrs = values.filter((v) => v.includes('/'));
		await run(
			'whitelist',
			'Change the CrowdSec whitelist',
			() => api.crowdsecWhitelistSave(ips, cidrs),
			'Whitelist saved; CrowdSec reloaded',
			whitelists
		);
	}

	async function addWhitelist(): Promise<void> {
		const value = await prompt({
			title: 'Add to whitelist',
			label: 'IP address or CIDR range',
			placeholder: '203.0.113.10 or 10.0.0.0/8',
			validate: (v) =>
				/^[0-9a-fA-F:.]+(\/\d{1,3})?$/.test(v.trim()) ? null : 'Enter an IP address or a CIDR range.'
		});
		if (!value) return;
		const managed = whitelist.filter((e) => e.source === 'jarvis').map((e) => e.value);
		await saveManaged([...new Set([...managed, value.trim()])]);
	}

	async function removeWhitelist(entry: WhitelistEntry): Promise<void> {
		if (entry.source === 'jarvis') {
			await saveManaged(
				whitelist.filter((e) => e.source === 'jarvis' && e.value !== entry.value).map((e) => e.value)
			);
		} else if (entry.source === 'lapi') {
			await run(
				'whitelist',
				'Change a CrowdSec allowlist',
				() => api.crowdsecAllowlistEdit(entry.origin, entry.value, false),
				'Removed from the allowlist',
				whitelists
			);
		}
	}

	// ------------------------------------------------------------ alerts, bouncers, metrics, hub
	let alertDetail = $state<{ alert: Alert; json: string } | null>(null);
	const alertColumns: Column<Alert>[] = [
		{ key: 'id', label: 'ID', value: (a) => Number(a.id), align: 'right', class: 'w-20 tabular' },
		{ key: 'source', label: 'Source', value: (a) => a.source, class: 'w-56' },
		{ key: 'scenario', label: 'Scenario', value: (a) => a.scenario, class: 'max-w-0 w-full' },
		{ key: 'events', label: 'Events', value: (a) => a.events, align: 'right', class: 'w-24 tabular' },
		{ key: 'createdAt', label: 'Created', value: (a) => a.createdAt, class: 'w-56' }
	];
	let alertSort = $state<Sort | null>({ key: 'id', dir: 'desc' });

	async function openAlert(alert: Alert): Promise<void> {
		try {
			alertDetail = {
				alert,
				json: await sudo.describe('Read a CrowdSec alert', () => api.crowdsecAlertDetail(alert.id))
			};
		} catch (error) {
			toast.error(error);
		}
	}

	let newKey = $state<{ name: string; key: string } | null>(null);
	const bouncerColumns: Column<Bouncer>[] = [
		{ key: 'name', label: 'Name', value: (b) => b.name, class: 'max-w-0 w-full' },
		{ key: 'ip', label: 'IP', value: (b) => b.ip, class: 'w-48' },
		{ key: 'kind', label: 'Type', value: (b) => b.kind, class: 'w-64 max-w-64' },
		{ key: 'version', label: 'Version', value: (b) => b.version, class: 'w-32' },
		{ key: 'lastPull', label: 'Last activity', value: (b) => b.lastPull, class: 'w-48' }
	];

	async function registerBouncer(): Promise<void> {
		const name = await prompt({
			title: 'Register bouncer',
			label: 'Name',
			placeholder: 'firewall-bouncer',
			validate: (v) => (/^[A-Za-z0-9_.@-]+$/.test(v) ? null : 'Use letters, digits, “-”, “_” and “.”.')
		});
		if (!name) return;
		try {
			const key = await sudo.describe('Register a CrowdSec bouncer', () => api.crowdsecBouncerAdd(name));
			newKey = { name, key };
			await bouncers.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	let tailFor = $state<Acquisition | null>(null);
	let tailText = $state('');
	async function loadTail(): Promise<void> {
		if (!tailFor) return;
		try {
			tailText = await api.crowdsecSourceTail(tailFor.source, 200);
		} catch (error) {
			toast.error(error);
		}
	}
	poll(() => visible && tailFor !== null, 5000, loadTail);

	let hubKind = $state('collections');
	let hubInstalledOnly = $state(false);
	const hubColumns: Column<HubItem>[] = [
		{ key: 'name', label: 'Name', value: (h) => h.name, class: 'w-96 max-w-96' },
		{ key: 'status', label: 'Status', value: (h) => h.status, class: 'w-48' },
		{ key: 'localVersion', label: 'Version', value: (h) => h.localVersion, class: 'w-24' },
		{ key: 'description', label: 'Description', value: (h) => h.description, class: 'max-w-0 w-full' }
	];
	const hubRows = $derived(
		(hub.data ?? []).filter((h) => h.kind === hubKind && (!hubInstalledOnly || h.installed))
	);
	const hubAct = (item: HubItem, action: HubAction) =>
		run(
			`${item.kind}/${item.name}`,
			`${action} ${item.name}`,
			() => api.crowdsecHubAction(item.kind, item.name, action),
			`${item.name}: ${action} done`,
			hub
		);

	export function refresh(): void {
		void status.refresh();
		for (const r of needs[view]) void r.refresh();
	}
</script>

{#snippet card(title: string, value: string | number, target: View, tone = '')}
	<button
		type="button"
		class="rounded-xl border bg-card p-4 text-left transition-colors hover:border-primary/40"
		onclick={() => (view = target)}
	>
		<p class="text-xs text-muted-foreground">{title}</p>
		<p class={cn('text-2xl font-semibold tabular', tone)}>{value}</p>
	</button>
{/snippet}

{#snippet ranking(title: string, rows: [string, number][], ip = false)}
	<section class="rounded-xl border bg-card p-4">
		<h3 class="mb-2 text-sm font-semibold">{title}</h3>
		{#each rows as [label, count] (label)}
			<div class="flex items-center gap-2 border-t py-1 text-sm first:border-t-0">
				{#if ip}<IpLink ip={label} class="min-w-0 flex-1 truncate text-xs" />{:else}<span
						class="selectable min-w-0 flex-1 truncate">{label}</span
					>{/if}
				<span class="text-xs text-muted-foreground tabular">{count}</span>
			</div>
		{:else}
			<p class="text-xs text-muted-foreground">Nothing yet.</p>
		{/each}
	</section>
{/snippet}

{#if status.error && !status.loaded}
	<StateView kind="error" error={status.error} onretry={status.refresh}>
		<Button variant="outline" size="sm" onclick={openConfig}>Connection settings</Button>
	</StateView>
{:else if !status.loaded}
	<StateView kind="loading" title="Looking for CrowdSec…" />
{:else if !ready}
	<div class="mx-auto flex h-full max-w-2xl flex-col justify-center gap-4 p-6">
		<div>
			<h2 class="text-base font-semibold">CrowdSec was not found</h2>
			<p class="text-sm text-muted-foreground">
				Jarvis looks for a native <code>cscli</code> and for a running container with a CrowdSec image. Install
				it below, or point Jarvis at your setup.
			</p>
		</div>
		<DependencyGuard tools={['crowdsec', 'crowdsecFirewallBouncer']} inline active={visible}>
			<p class="text-sm text-success">CrowdSec and the firewall bouncer are installed.</p>
		</DependencyGuard>
		<div class="rounded-xl border bg-card p-3 text-sm">
			<p class="font-medium">Running CrowdSec in Docker?</p>
			<p class="text-xs text-muted-foreground">
				Start the official image (<code class="selectable">crowdsecurity/crowdsec</code>) with your logs
				mounted, then choose “Docker container” in the connection settings.
			</p>
		</div>
		<div class="flex gap-2">
			<Button variant="outline" size="sm" onclick={() => status.refresh()}>Re-check</Button>
			<Button variant="outline" size="sm" onclick={openConfig}><Settings /> Connection settings</Button>
		</div>
	</div>
{:else}
	<Page scroll={view === 'dashboard'}>
		{#snippet toolbar()}
			{#if view !== 'dashboard'}<SearchInput bind:value={search} placeholder="Search…" class="w-56" />{/if}
			{#if view === 'decisions'}
				<Button
					size="sm"
					onclick={() => {
						ban = { scope: 'ip', value: '', duration: '4h', custom: '', reason: '' };
						banOpen = true;
					}}><Plus /> Add ban</Button
				>
				<Button variant="outline" size="sm" disabled={!decisions.data?.length} onclick={unbanAll}
					>Unban all</Button
				>
			{:else if view === 'whitelist'}
				<Button size="sm" onclick={addWhitelist}><Plus /> Add to whitelist</Button>
			{:else if view === 'bouncers'}
				<Button size="sm" onclick={registerBouncer}><Plus /> Register bouncer</Button>
				<Button
					variant="outline"
					size="sm"
					onclick={() =>
						run(
							'prune',
							'Prune CrowdSec bouncers',
							() => api.crowdsecBouncersPrune(),
							'Inactive bouncers pruned',
							bouncers
						)}>Prune inactive</Button
				>
			{:else if view === 'hub'}
				<SelectField
					bind:value={hubKind}
					class="w-40"
					options={['collections', 'parsers', 'scenarios', 'postoverflows']}
				/>
				<Button
					variant={hubInstalledOnly ? 'secondary' : 'outline'}
					size="sm"
					onclick={() => (hubInstalledOnly = !hubInstalledOnly)}>Installed only</Button
				>
				<Button
					variant="outline"
					size="sm"
					onclick={() =>
						run(
							'hub-update',
							'Update the CrowdSec hub index',
							() => api.crowdsecHubUpdate(),
							'Hub index updated',
							hub
						)}>Update hub</Button
				>
			{/if}
			<span class="flex-1"></span>
			<span class="text-xs text-muted-foreground">
				{status.data?.mode === 'docker' ? `container ${status.data.container}` : status.data?.mode} · {status
					.data?.version}
			</span>
			<IconButton label="Connection settings" onclick={openConfig}><Settings /></IconButton>
			<RefreshControl onrefresh={refresh} loading={needs[view].some((r) => r.loading)} />
		{/snippet}
		{#snippet header()}
			<SubTabs
				bind:value={view}
				items={[
					{ id: 'dashboard', label: 'Dashboard' },
					{ id: 'decisions', label: 'Decisions', count: decisions.data?.length },
					{ id: 'whitelist', label: 'Whitelist', count: whitelists.loaded ? whitelist.length : null },
					{ id: 'alerts', label: 'Alerts', count: alerts.data?.length },
					{ id: 'bouncers', label: 'Bouncers', count: bouncers.data?.length },
					{ id: 'metrics', label: 'Metrics' },
					{ id: 'hub', label: 'Hub' }
				]}
			/>
		{/snippet}

		{#if view === 'dashboard'}
			<section class="mb-3 flex flex-wrap items-center gap-3 rounded-xl border bg-card p-4">
				<span
					class={cn(
						'size-2.5 rounded-full',
						status.data?.serviceActive === false ? 'bg-destructive' : 'bg-success'
					)}
				></span>
				<div class="mr-auto">
					<p class="text-sm font-medium">
						CrowdSec {status.data?.serviceActive === false ? 'is stopped' : 'is running'}
					</p>
					<p class="text-xs text-muted-foreground">
						Version {status.data?.version || 'unknown'}{parsing !== null
							? ` · ${parsing.toFixed(0)}% of log lines parsed`
							: ''}
					</p>
				</div>
				<Button variant="outline" size="sm" disabled={busy.has('service')} onclick={() => service('start')}
					>Start</Button
				>
				<Button variant="outline" size="sm" disabled={busy.has('service')} onclick={() => service('stop')}
					>Stop</Button
				>
				<Button variant="outline" size="sm" disabled={busy.has('service')} onclick={() => service('restart')}
					>Restart</Button
				>
			</section>
			<div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
				{@render card('Active bans', decisions.data?.length ?? '—', 'decisions', 'text-destructive')}
				{@render card('Bouncers', bouncers.data?.length ?? '—', 'bouncers')}
				{@render card('Alerts', alerts.data?.length ?? '—', 'alerts', 'text-warning')}
				{@render card('Whitelist entries', whitelists.loaded ? whitelist.length : '—', 'whitelist')}
			</div>
			<div class="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2 xl:grid-cols-4">
				{@render ranking(
					'Top scenarios',
					top(alerts.data ?? [], (a) => a.scenario)
				)}
				{@render ranking(
					'Top attacking IPs',
					top(alerts.data ?? [], (a) => a.source),
					true
				)}
				{@render ranking(
					'Top countries',
					top(alerts.data ?? [], (a) => (a.country ? `${flag(a.country)} ${a.country}` : ''))
				)}
				{@render ranking(
					'Ban origins',
					top(decisions.data ?? [], (d) => d.origin)
				)}
			</div>
		{:else if view === 'decisions'}
			<DataTable
				rows={decisions.data ?? []}
				columns={decisionColumns}
				rowKey={(d) => d.id}
				{search}
				bind:sort={decisionSort}
				busy={busy.keys}
				loading={decisions.loading}
				error={decisions.error}
				onretry={decisions.refresh}
				empty="No active decisions"
				class="flex-1"
			>
				{#snippet cell(d, column)}
					{#if column.key === 'value'}<IpLink ip={d.value} class="text-xs" />
					{:else if column.key === 'country'}{flag(d.country)}
						{d.country} <span class="text-muted-foreground">{d.asName}</span>
					{:else if column.key === 'until'}{d.until ? formatDateTime(d.until) : '—'}
					{:else}{column.value?.(d)}{/if}
				{/snippet}
				{#snippet actions(d)}
					<Button
						variant="ghost"
						size="xs"
						onclick={() =>
							run(d.id, `Unban ${d.value}`, () => api.crowdsecUnban(d.id), `${d.value} unbanned`, decisions)}
						>Unban</Button
					>
				{/snippet}
			</DataTable>
		{:else if view === 'whitelist'}
			<DataTable
				rows={whitelist}
				columns={[
					{ key: 'value', label: 'IP / range', value: (e) => e.value, class: 'w-72' },
					{ key: 'source', label: 'Source', value: (e) => SOURCE_LABEL[e.source], class: 'w-40' },
					{ key: 'origin', label: 'Defined in', value: (e) => e.origin, mono: true, class: 'max-w-0 w-full' }
				]}
				rowKey={(e) => `${e.source}|${e.origin}|${e.value}`}
				{search}
				loading={whitelists.loading}
				error={whitelists.error}
				onretry={whitelists.refresh}
				empty="Nothing is whitelisted"
				class="flex-1"
			>
				{#snippet cell(e, column)}
					{#if column.key === 'value'}<IpLink ip={e.value} class="text-xs" />
					{:else if column.key === 'source'}<span
							class={cn(
								'rounded px-1.5 py-0.5 text-xs',
								e.source === 'jarvis' ? 'bg-primary/15 text-primary' : 'bg-muted text-muted-foreground'
							)}>{SOURCE_LABEL[e.source]}</span
						>
					{:else}{e.origin}{/if}
				{/snippet}
				{#snippet actions(e)}
					{#if e.source === 'jarvis' || e.source === 'lapi'}
						<IconButton label="Remove" onclick={() => removeWhitelist(e)}><Trash2 /></IconButton>
					{:else}
						<span class="px-2 text-xs text-muted-foreground">read-only</span>
					{/if}
				{/snippet}
			</DataTable>
		{:else if view === 'alerts'}
			<DataTable
				rows={alerts.data ?? []}
				columns={alertColumns}
				rowKey={(a) => a.id}
				{search}
				bind:sort={alertSort}
				loading={alerts.loading}
				error={alerts.error}
				onretry={alerts.refresh}
				empty="No alerts"
				onrowdblclick={openAlert}
				class="flex-1"
			>
				{#snippet cell(a, column)}
					{#if column.key === 'source'}{flag(a.country)} <IpLink ip={a.source} class="text-xs" />
					{:else if column.key === 'createdAt'}{formatDateTime(a.createdAt)}
						<span class="text-xs text-muted-foreground">· {formatRelative(a.createdAt)}</span>
					{:else if column.key === 'id'}{a.id}
					{:else}{column.value?.(a)}{/if}
				{/snippet}
				{#snippet actions(a)}
					<Button variant="ghost" size="xs" onclick={() => openAlert(a)}>Details</Button>
				{/snippet}
			</DataTable>
		{:else if view === 'bouncers'}
			<DataTable
				rows={bouncers.data ?? []}
				columns={bouncerColumns}
				rowKey={(b) => b.name}
				{search}
				busy={busy.keys}
				loading={bouncers.loading}
				error={bouncers.error}
				onretry={bouncers.refresh}
				empty="No bouncers registered"
				emptyHint="Without a bouncer, decisions are recorded but nothing is blocked."
				class="flex-1"
			>
				{#snippet cell(b, column)}
					{#if column.key === 'ip'}<IpLink ip={b.ip} class="text-xs" />
					{:else if column.key === 'lastPull'}{b.lastPull ? formatRelative(b.lastPull) : 'never'}
					{:else}{column.value?.(b)}{/if}
				{/snippet}
				{#snippet actions(b)}
					<IconButton
						label="Delete bouncer"
						onclick={async () => {
							if (
								await confirm({
									title: `Delete bouncer “${b.name}”?`,
									message: 'It will no longer be able to fetch decisions.',
									confirmLabel: 'Delete',
									destructive: true
								})
							) {
								await run(
									b.name,
									'Delete a CrowdSec bouncer',
									() => api.crowdsecBouncerDelete(b.name),
									'Bouncer deleted',
									bouncers
								);
							}
						}}
					>
						<Trash2 />
					</IconButton>
				{/snippet}
			</DataTable>
		{:else if view === 'metrics'}
			<div class="flex min-h-0 flex-1 gap-3">
				<div class="min-h-0 min-w-0 flex-1 overflow-y-auto">
					{#if metrics.error && !metrics.loaded}
						<StateView kind="error" error={metrics.error} onretry={metrics.refresh} />
					{:else}
						<table class="w-full overflow-hidden rounded-lg border bg-card text-sm">
							<thead class="text-xs text-muted-foreground">
								<tr
									><th class="px-3 py-2 text-left font-medium">Source</th><th
										class="px-3 py-2 text-right font-medium">Read</th
									><th class="px-3 py-2 text-right font-medium">Parsed</th><th
										class="px-3 py-2 text-right font-medium">Unparsed</th
									><th class="w-48 px-3 py-2 text-left font-medium">Success</th></tr
								>
							</thead>
							<tbody>
								{#each (metrics.data ?? []).filter((a) => !search || a.source.includes(search)) as a (a.source)}
									{@const rate = a.read ? (a.parsed / a.read) * 100 : 0}
									<tr
										class={cn(
											'cursor-pointer border-t hover:bg-muted/40',
											tailFor?.source === a.source && 'bg-primary/8'
										)}
										onclick={() => {
											tailFor = a;
											tailText = '';
											void loadTail();
										}}
									>
										<td class="selectable px-3 py-1.5 font-mono text-xs">{a.source}</td>
										<td class="px-3 py-1.5 text-right tabular">{a.read.toLocaleString()}</td>
										<td class="px-3 py-1.5 text-right tabular">{a.parsed.toLocaleString()}</td>
										<td class="px-3 py-1.5 text-right tabular">{a.unparsed.toLocaleString()}</td>
										<td class="px-3 py-1.5">
											<div class="flex items-center gap-2">
												<div class="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
													<div
														class={cn('h-full', rate < 20 ? 'bg-warning' : 'bg-success')}
														style="width: {rate}%"
													></div>
												</div>
												<span class="w-10 text-right text-xs tabular">{rate.toFixed(0)}%</span>
											</div>
										</td>
									</tr>
								{:else}
									<tr
										><td colspan="5" class="p-4 text-center text-sm text-muted-foreground"
											>{metrics.loaded ? 'No acquisition metrics yet.' : 'Loading…'}</td
										></tr
									>
								{/each}
							</tbody>
						</table>
						<p class="mt-2 text-xs text-muted-foreground">
							Select a file source to preview its latest lines.
						</p>
					{/if}
				</div>
				{#if tailFor}
					<LogViewer
						source={tailText}
						live
						severity
						placeholder="Loading…"
						downloadName="source.log"
						class="w-1/2"
					/>
				{/if}
			</div>
		{:else}
			<DataTable
				rows={hubRows}
				columns={hubColumns}
				rowKey={(h) => `${h.kind}/${h.name}`}
				{search}
				busy={busy.keys}
				loading={hub.loading}
				error={hub.error}
				onretry={hub.refresh}
				empty="Nothing in this category"
				class="flex-1"
			>
				{#snippet cell(h, column)}
					{#if column.key === 'status'}
						<span
							class={h.installed
								? h.status.includes('update') || h.status.includes('outdated')
									? 'text-warning'
									: 'text-success'
								: 'text-muted-foreground'}>{h.status || 'not installed'}</span
						>
					{:else if column.key === 'name'}
						<span class="font-mono text-xs">{h.name}</span>
					{:else}{column.value?.(h)}{/if}
				{/snippet}
				{#snippet actions(h)}
					{#if h.installed}
						<Button variant="ghost" size="xs" onclick={() => hubAct(h, 'upgrade')}>Upgrade</Button>
						<Button variant="ghost" size="xs" onclick={() => hubAct(h, 'remove')}>Remove</Button>
					{:else}
						<Button variant="outline" size="xs" onclick={() => hubAct(h, 'install')}>Install</Button>
					{/if}
				{/snippet}
			</DataTable>
		{/if}
	</Page>
{/if}

<Modal bind:open={banOpen} title="Add ban" size="md">
	<form
		id="ban-form"
		class="grid grid-cols-2 gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void addBan();
		}}
	>
		<Field label="Scope"
			><SelectField
				bind:value={ban.scope}
				options={[
					{ value: 'ip', label: 'Single IP' },
					{ value: 'range', label: 'Range (CIDR)' }
				]}
			/></Field
		>
		<Field label={ban.scope === 'ip' ? 'IP address' : 'Range'} required
			><Input
				bind:value={ban.value}
				class="font-mono"
				placeholder={ban.scope === 'ip' ? '203.0.113.10' : '203.0.113.0/24'}
				spellcheck="false"
				autofocus
			/></Field
		>
		<Field label="Duration"
			><SelectField
				bind:value={ban.duration}
				options={[
					{ value: '1h', label: '1 hour' },
					{ value: '4h', label: '4 hours' },
					{ value: '24h', label: '24 hours' },
					{ value: '48h', label: '48 hours' },
					{ value: '7d', label: '7 days' },
					{ value: 'custom', label: 'Custom…' }
				]}
			/></Field
		>
		{#if ban.duration === 'custom'}
			<Field label="Custom duration" hint="e.g. 30m, 12h, 30d"
				><Input bind:value={ban.custom} class="font-mono" /></Field
			>
		{/if}
		<Field label="Reason" class="col-span-2"><Input bind:value={ban.reason} placeholder="manual ban" /></Field
		>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (banOpen = false)}>Cancel</Button>
		<Button
			type="submit"
			form="ban-form"
			variant="destructive"
			disabled={!ban.value.trim() || busy.has('ban')}>Ban</Button
		>
	{/snippet}
</Modal>

{#if alertDetail}
	{@const a = alertDetail.alert}
	<Modal open title="Alert {a.id}" size="xl" class="h-[80vh]" flush onclose={() => (alertDetail = null)}>
		<dl class="selectable mb-3 grid shrink-0 grid-cols-[7rem_1fr] gap-x-4 gap-y-1 text-sm">
			<dt class="text-muted-foreground">Scenario</dt>
			<dd>{a.scenario}</dd>
			<dt class="text-muted-foreground">Attacker</dt>
			<dd>
				{flag(a.country)}
				<IpLink ip={a.source} class="text-xs" /> <span class="text-muted-foreground">{a.asName}</span>
			</dd>
			<dt class="text-muted-foreground">Events</dt>
			<dd>{a.events}</dd>
			<dt class="text-muted-foreground">Message</dt>
			<dd>{a.message || '—'}</dd>
		</dl>
		<LogViewer source={alertDetail.json} downloadName="alert-{a.id}.json" class="flex-1" />
	</Modal>
{/if}

{#if newKey}
	<Modal
		open
		title="Bouncer “{newKey.name}” registered"
		description="This API key is shown only once. Copy it into the bouncer's configuration now."
		size="md"
		onclose={() => (newKey = null)}
	>
		<div class="flex items-center gap-2 rounded-md border bg-sunken p-2">
			<code class="selectable min-w-0 flex-1 text-xs break-all">{newKey.key}</code>
			<IconButton
				label="Copy key"
				onclick={() => copyText(newKey?.key ?? '').then(() => toast.success('API key copied'))}
				><Copy /></IconButton
			>
		</div>
		{#snippet footer()}
			<Button onclick={() => (newKey = null)}>Done</Button>
		{/snippet}
	</Modal>
{/if}

<Modal
	bind:open={configOpen}
	title="CrowdSec connection"
	description="How Jarvis reaches cscli on this server."
	size="md"
>
	<div class="flex flex-col gap-3">
		<Field label="Mode">
			<SelectField
				bind:value={config.mode}
				options={[
					{ value: 'auto', label: 'Detect automatically' },
					{ value: 'native', label: 'Installed on the server (cscli)' },
					{ value: 'docker', label: 'Docker container' },
					{ value: 'custom', label: 'Custom command' }
				]}
			/>
		</Field>
		{#if config.mode === 'docker'}
			<Field label="Container name" required
				><Input bind:value={config.container} placeholder="crowdsec" spellcheck="false" /></Field
			>
		{:else if config.mode === 'custom'}
			<Field
				label="cscli command"
				required
				hint="The full command that runs cscli, e.g. podman exec crowdsec cscli"
			>
				<Input bind:value={config.prefix} class="font-mono text-xs" spellcheck="false" />
			</Field>
		{/if}
	</div>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (configOpen = false)}>Cancel</Button>
		<Button onclick={saveConfig}>Save and test</Button>
	{/snippet}
</Modal>
