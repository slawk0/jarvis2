<script lang="ts">
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import IpLink from '$lib/components/IpLink.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		api,
		toIpcError,
		type IpcError,
		type LoginRecord,
		type LoginSession,
		type LogQuery
	} from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { loadDoc, saveDoc, type LogSource } from '$lib/services/profile-data';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { resource } from '$lib/state/resource.svelte';
	import { debounce, uid } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';
	import { workspace } from '$lib/workspace/workspace.svelte';

	let { visible }: TabProps = $props();

	let view = $state<'viewer' | 'sessions'>('viewer');

	// ------------------------------------------------------------ viewer
	const sources = resource((io) => io.logsSources());
	let custom = $state<LogSource[]>([]);
	let sourceId = $state('journal');
	let unit = $state('');
	let priority = $state('');
	let lines = $state('500');
	let job = $state<Job | null>(null);
	let followError = $state<IpcError | null>(null);
	let addOpen = $state(false);
	let newPath = $state('/var/log/');
	let appliedUnit = $state('');
	/** Bumped to restart the stream after an error. */
	let attempt = $state(0);

	const options = $derived([
		...(sources.data ?? []).map((s) => ({
			value: s.id,
			label: s.available ? s.label : `${s.label} (not available)`,
			disabled: !s.available
		})),
		...custom.map((c) => ({ value: `custom:${c.id}`, label: c.path }))
	]);
	const query = $derived.by((): LogQuery | null => {
		if (sourceId === 'journal') return { kind: 'journal', unit: appliedUnit, priority };
		if (sourceId.startsWith('custom:')) {
			const path = custom.find((c) => `custom:${c.id}` === sourceId)?.path;
			return path ? { kind: 'file', path } : null;
		}
		const path = sources.data?.find((s) => s.id === sourceId)?.path;
		return path ? { kind: 'file', path } : null;
	});

	// Typing a unit name should not restart the stream on every key press.
	const applyUnit = debounce((value: string) => (appliedUnit = value.trim()), 500);
	$effect(() => applyUnit(unit));

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void sources.refresh().then(() => {
				// Without a journal, start on the first file that exists.
				if (sources.data && !sources.data[0]?.available)
					sourceId = sources.data.find((s) => s.available)?.id ?? sourceId;
			});
			loadDoc('logSources', []).then(
				(list) => (custom = list),
				() => {}
			);
		}
	});

	// "Show logs of this unit" from another tab.
	$effect(() => {
		if (!visible) return;
		const request = workspace.takeRequest('logs');
		if (!request) return;
		view = 'viewer';
		sourceId = 'journal';
		unit = request.unit;
		appliedUnit = request.unit;
	});

	// The live stream: restarted whenever the query changes, stopped while hidden.
	$effect(() => {
		const current = query;
		const count = Number(lines);
		void attempt;
		if (!visible || view !== 'viewer' || !current || !sources.loaded) return;
		let active: Job | null = null;
		let cancelled = false;
		followError = null;
		api.logsFollow(current, count).then(
			(id) => {
				active = jobs.get(id);
				if (cancelled) void active.stop();
				else job = active;
			},
			(raw) => {
				followError = toIpcError(raw);
			}
		);
		return () => {
			cancelled = true;
			void active?.stop();
			job = null;
		};
	});

	async function addCustom(): Promise<void> {
		const path = newPath.trim();
		if (!path.startsWith('/') || path.endsWith('/')) return;
		const entry = { id: uid(), name: path, path };
		custom = [...custom, entry];
		addOpen = false;
		sourceId = `custom:${entry.id}`;
		await saveDoc('logSources', $state.snapshot(custom)).catch((e) => toast.error(e));
	}

	async function removeCustom(): Promise<void> {
		custom = custom.filter((c) => `custom:${c.id}` !== sourceId);
		sourceId = 'journal';
		await saveDoc('logSources', $state.snapshot(custom)).catch((e) => toast.error(e));
	}

	// ------------------------------------------------------------ sessions
	const sessions = resource((io) => io.sessionsList());
	const failed = resource((io) => io.sessionsFailed());
	let showFailed = $state(false);
	$effect(() => {
		if (visible && view === 'sessions' && !sessions.loaded && !sessions.loading) void sessions.refresh();
	});

	const sessionColumns: Column<LoginSession>[] = [
		{ key: 'user', label: 'User', value: (s) => s.user, class: 'w-48' },
		{ key: 'tty', label: 'Terminal', value: (s) => s.tty, mono: true, class: 'w-32' },
		{ key: 'from', label: 'From', value: (s) => s.from, class: 'w-64' },
		{ key: 'login', label: 'Logged in', value: (s) => s.login, class: 'max-w-0 w-full tabular' }
	];
	const recordColumns: Column<LoginRecord>[] = [
		{ key: 'user', label: 'User', value: (r) => r.user, class: 'w-48' },
		{ key: 'tty', label: 'Terminal', value: (r) => r.tty, mono: true, class: 'w-32' },
		{ key: 'from', label: 'From', value: (r) => r.from, class: 'w-64' },
		{ key: 'when', label: 'When', value: (r) => r.when, class: 'max-w-0 w-full tabular' }
	];

	async function kick(session: LoginSession): Promise<void> {
		const ok = await confirm({
			title: `End the session of ${session.user} on ${session.tty}?`,
			message: 'Everything running in that terminal is killed immediately.',
			confirmLabel: 'Kick session',
			destructive: true
		});
		if (!ok) return;
		try {
			await sudo.describe(`Kick ${session.user} (${session.tty})`, () => api.sessionKick(session.tty));
			toast.success('Session ended');
			await sessions.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	async function toggleFailed(): Promise<void> {
		showFailed = !showFailed;
		if (showFailed) await failed.refresh();
	}

	export function refresh(): void {
		if (view === 'sessions') {
			void sessions.refresh();
			if (showFailed) void failed.refresh();
		} else {
			void sources.refresh();
		}
	}
</script>

<Page scroll={view === 'sessions'}>
	{#snippet header()}
		<SubTabs
			bind:value={view}
			items={[
				{ id: 'viewer', label: 'Log viewer' },
				{ id: 'sessions', label: 'Sessions', count: sessions.data?.current.length }
			]}
		/>
	{/snippet}

	{#if view === 'viewer'}
		<div class="mb-2 flex flex-wrap items-center gap-2">
			<SelectField bind:value={sourceId} {options} class="w-72" />
			{#if sourceId === 'journal'}
				<Input bind:value={unit} placeholder="Unit (all)" class="w-52 font-mono text-xs" spellcheck="false" />
				<SelectField
					bind:value={priority}
					class="w-44"
					options={[
						{ value: '', label: 'All priorities' },
						{ value: 'err', label: 'Errors and worse' },
						{ value: 'warning', label: 'Warnings and worse' },
						{ value: 'notice', label: 'Notices and worse' },
						{ value: 'info', label: 'Info and worse' }
					]}
				/>
			{/if}
			<SelectField
				bind:value={lines}
				class="w-36"
				options={[
					{ value: '100', label: 'Last 100 lines' },
					{ value: '500', label: 'Last 500 lines' },
					{ value: '2000', label: 'Last 2,000 lines' },
					{ value: '10000', label: 'Last 10,000 lines' }
				]}
			/>
			<span class="flex-1"></span>
			{#if sourceId.startsWith('custom:')}
				<IconButton label="Remove this log file from the list" onclick={removeCustom}><Trash2 /></IconButton>
			{/if}
			<Button
				variant="outline"
				size="sm"
				onclick={() => {
					newPath = '/var/log/';
					addOpen = true;
				}}><Plus /> Add log file</Button
			>
		</div>
		{#if followError}
			<StateView
				kind="error"
				error={followError}
				title={followError.code === 'NOT_FOUND'
					? 'This log file does not exist'
					: followError.code === 'UNSUPPORTED'
						? 'The systemd journal is not available on this server'
						: undefined}
				onretry={() => attempt++}
			/>
		{:else}
			<LogViewer
				source={job}
				severity
				live
				maxLines={50_000}
				placeholder="Waiting for log lines…"
				downloadName="{sourceId.replace(/[^a-z0-9-]/gi, '_')}.log"
				class="flex-1"
			/>
		{/if}
	{:else}
		<div class="mb-2 flex items-center gap-2">
			<h3 class="text-sm font-semibold">Logged-in users</h3>
			<span class="flex-1"></span>
			<RefreshControl onrefresh={sessions.refresh} loading={sessions.loading} />
		</div>
		<DataTable
			rows={sessions.data?.current ?? []}
			columns={sessionColumns}
			rowKey={(s) => `${s.user}|${s.tty}`}
			loading={sessions.loading}
			error={sessions.error}
			onretry={sessions.refresh}
			empty="Nobody is logged in on a terminal"
			class="max-h-72"
		>
			{#snippet cell(s, column)}
				{#if column.key === 'from'}<IpLink ip={s.from} class="text-xs" />
				{:else if column.key === 'user'}{s.user}{#if s.own}<span
							class="ml-2 rounded bg-primary/15 px-1 text-[10px] text-primary">this Jarvis</span
						>{/if}
				{:else}{column.value?.(s)}{/if}
			{/snippet}
			{#snippet actions(s)}
				<Button variant="ghost" size="xs" disabled={s.own} onclick={() => kick(s)}>Kick</Button>
			{/snippet}
		</DataTable>

		<h3 class="mt-5 mb-2 text-sm font-semibold">Recent logins</h3>
		<DataTable
			rows={sessions.data?.recent ?? []}
			columns={recordColumns}
			rowKey={(r) => `${r.user}|${r.tty}|${r.when}`}
			loading={sessions.loading}
			empty="No login history"
			class="max-h-80"
		>
			{#snippet cell(r, column)}
				{#if column.key === 'from'}<IpLink ip={r.from} class="text-xs" />{:else}{column.value?.(r)}{/if}
			{/snippet}
		</DataTable>

		<div class="mt-5 mb-2 flex items-center gap-3">
			<h3 class="text-sm font-semibold">Failed login attempts</h3>
			<Button variant="outline" size="xs" onclick={toggleFailed}
				>{showFailed ? 'Hide' : 'Show (needs root)'}</Button
			>
		</div>
		{#if showFailed}
			<DataTable
				rows={failed.data ?? []}
				columns={recordColumns}
				rowKey={(r) => `${r.user}|${r.tty}|${r.when}|${r.from}`}
				loading={failed.loading}
				error={failed.error}
				onretry={failed.refresh}
				empty="No failed logins recorded"
				class="max-h-80"
			>
				{#snippet cell(r, column)}
					{#if column.key === 'from'}<IpLink ip={r.from} class="text-xs" />{:else}{column.value?.(r)}{/if}
				{/snippet}
			</DataTable>
		{/if}
	{/if}
</Page>

<Modal
	bind:open={addOpen}
	title="Add a log file"
	description="The file is remembered for this server."
	size="md"
>
	<PathInput bind:value={newPath} autofocus onsubmit={addCustom} />
	{#snippet footer()}
		<Button variant="outline" onclick={() => (addOpen = false)}>Cancel</Button>
		<Button disabled={!newPath.trim().startsWith('/') || newPath.trim().endsWith('/')} onclick={addCustom}
			>Add</Button
		>
	{/snippet}
</Modal>
