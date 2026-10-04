<!-- Restic: repositories per server, repository actions and the snapshot list. -->
<script lang="ts">
	import Archive from '@lucide/svelte/icons/archive';
	import FolderOpen from '@lucide/svelte/icons/folder-open';
	import History from '@lucide/svelte/icons/history';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Play from '@lucide/svelte/icons/play';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import JobDialog from '$lib/components/JobDialog.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { formatBytes, formatDateTime, formatNumber, formatRelative } from '$lib/format';
	import {
		api,
		toIpcError,
		type IpcError,
		type KeepPolicy,
		type RepoStats,
		type RepoStatus,
		type ResticRepo,
		type Snapshot
	} from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { resource } from '$lib/state/resource.svelte';
	import type { TabProps } from '$lib/workspace/registry';
	import RepoDialog, { blankRepo, KINDS } from './RepoDialog.svelte';
	import SnapshotBrowser from './SnapshotBrowser.svelte';

	let { visible }: TabProps = $props();

	const PATHS_PLACEHOLDER = '/etc\n/var/www';

	const repos = resource((io) => io.resticRepos());
	let selectedId = $state('');
	let editing = $state<ResticRepo | null>(null);

	let status = $state<RepoStatus | null>(null);
	let statusError = $state<IpcError | null>(null);
	let snapshots = $state<Snapshot[]>([]);
	let snapshotsError = $state<IpcError | null>(null);
	let loading = $state(false);
	let stats = $state<RepoStats | null>(null);
	let statsLoading = $state(false);
	let selection = $state(new Set<string>());
	let job = $state<Job | null>(null);
	let browsing = $state<Snapshot | null>(null);

	let backupOpen = $state(false);
	let backupPaths = $state('');
	let backupTags = $state('');
	let backupExcludes = $state('');
	let restoring = $state<Snapshot | null>(null);
	let restoreTarget = $state('');
	let policyOpen = $state(false);
	let policy = $state<KeepPolicy>({ last: 7, daily: 7, weekly: 4, monthly: 6 });

	const selected = $derived(repos.data?.find((r) => r.id === selectedId));
	const kindLabel = $derived(KINDS.find((k) => k.value === selected?.kind)?.label ?? '');

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void repos.refresh();
		}
	});
	$effect(() => {
		const list = repos.data;
		if (list?.length && !list.some((r) => r.id === selectedId)) selectedId = list[0].id;
	});

	let seq = 0;
	async function load(): Promise<void> {
		const id = selectedId;
		if (!id) return;
		const current = ++seq;
		loading = true;
		statusError = snapshotsError = null;
		try {
			const next = await api.resticStatus(id);
			if (current !== seq) return;
			status = next;
			if (next.initialized) {
				try {
					const list = await api.resticSnapshots(id);
					if (current === seq) snapshots = list;
				} catch (raw) {
					if (current === seq) snapshotsError = toIpcError(raw);
				}
			} else {
				snapshots = [];
			}
		} catch (raw) {
			if (current === seq) statusError = toIpcError(raw);
		} finally {
			if (current === seq) loading = false;
		}
	}

	// A different repository starts clean.
	let shownId = '';
	$effect(() => {
		if (!visible || selectedId === shownId) return;
		shownId = selectedId;
		status = null;
		stats = null;
		snapshots = [];
		selection = new Set();
		void load();
	});

	const columns: Column<Snapshot>[] = [
		{ key: 'id', label: 'ID', value: (s) => s.shortId, mono: true, class: 'w-28' },
		{ key: 'time', label: 'Time', value: (s) => s.time, class: 'w-56 tabular' },
		{ key: 'host', label: 'Host', value: (s) => s.hostname, class: 'w-40' },
		{ key: 'paths', label: 'Paths', value: (s) => s.paths.join(', '), mono: true, class: 'max-w-0 w-full' },
		{ key: 'tags', label: 'Tags', value: (s) => s.tags.join(', '), class: 'w-48 max-w-48' }
	];

	const lines = (text: string) =>
		text
			.split(/[\n,]/)
			.map((l) => l.trim())
			.filter(Boolean);

	/** Start a restic job, show its output and reload when it ends. */
	async function run(reason: string, start: () => Promise<string>): Promise<boolean> {
		try {
			const id = await sudo.describe(reason, start);
			job = jobs.get(id);
			const result = await job.wait();
			await load();
			return result.status === 'done';
		} catch (e) {
			toast.error(e);
			return false;
		}
	}

	async function init(): Promise<void> {
		if (!selected) return;
		await run('Initialise restic repository', () => api.resticInit(selected.id));
	}

	async function backup(): Promise<void> {
		if (!selected) return;
		const paths = lines(backupPaths);
		if (paths.some((p) => !p.startsWith('/'))) {
			toast.warning('Paths must be absolute (start with /).');
			return;
		}
		backupOpen = false;
		await run('Run restic backup', () =>
			api.resticBackup(selected.id, paths, lines(backupTags), lines(backupExcludes))
		);
	}

	async function loadStats(): Promise<void> {
		if (!selected) return;
		statsLoading = true;
		try {
			stats = await api.resticStats(selected.id);
		} catch (e) {
			toast.error(e, 'Could not read repository statistics');
		} finally {
			statsLoading = false;
		}
	}

	async function restore(): Promise<void> {
		const snapshot = restoring;
		if (!selected || !snapshot || !restoreTarget.trim().startsWith('/')) return;
		const target = restoreTarget.trim();
		restoring = null;
		await run(`Restore snapshot ${snapshot.shortId}`, () =>
			api.resticRestore(selected.id, snapshot.id, target)
		);
	}

	async function forget(list: Snapshot[]): Promise<void> {
		if (!selected || list.length === 0) return;
		const ok = await confirm({
			title: list.length === 1 ? `Forget snapshot ${list[0].shortId}?` : `Forget ${list.length} snapshots?`,
			message: 'The snapshots are removed and data no other snapshot needs is pruned. This cannot be undone.',
			detail: list.map((s) => `${s.shortId}  ${formatDateTime(s.time)}  ${s.paths.join(', ')}`).join('\n'),
			confirmLabel: 'Forget and prune',
			acknowledge: 'I understand that the removed snapshots cannot be recovered.',
			destructive: true
		});
		if (!ok) return;
		selection = new Set();
		await run('Forget restic snapshots', () =>
			api.resticForget(
				selected.id,
				list.map((s) => s.id)
			)
		);
	}

	async function applyPolicy(dryRun: boolean): Promise<void> {
		if (!selected) return;
		const keep = $state.snapshot(policy);
		if (!dryRun) {
			const ok = await confirm({
				title: 'Apply the retention policy?',
				message:
					'Every snapshot the policy does not keep is removed and its data pruned. Run a dry run first to see what would be removed.',
				confirmLabel: 'Forget and prune',
				acknowledge: 'I understand that the removed snapshots cannot be recovered.',
				destructive: true
			});
			if (!ok) return;
			policyOpen = false;
		}
		await run(dryRun ? 'Retention dry run' : 'Apply retention policy', () =>
			api.resticForgetPolicy(selected.id, keep, dryRun)
		);
	}

	async function removeRepo(): Promise<void> {
		if (!selected) return;
		const ok = await confirm({
			title: `Remove “${selected.name}” from Jarvis?`,
			message:
				'Only the saved configuration and its secrets are removed. The repository and its snapshots are not touched.',
			confirmLabel: 'Remove',
			destructive: true
		});
		if (!ok) return;
		try {
			await api.resticRepoDelete(selected.id);
			await repos.refresh();
		} catch (e) {
			toast.error(e);
		}
	}

	async function saved(repo: ResticRepo): Promise<void> {
		await repos.refresh();
		selectedId = repo.id;
		shownId = repo.id;
		void load();
	}

	export function refresh(): void {
		if (selected) void load();
		else void repos.refresh();
	}
</script>

{#if repos.error && !repos.loaded}
	<StateView kind="error" error={repos.error} onretry={repos.refresh} />
{:else if !repos.loaded}
	<StateView kind="loading" />
{:else if !repos.data?.length}
	<StateView
		kind="empty"
		icon={Archive}
		title="No restic repositories yet"
		message="Add a repository to create encrypted, deduplicated backups of this server."
	>
		<Button onclick={() => (editing = blankRepo())}><Plus /> Add a repository</Button>
	</StateView>
{:else if selected}
	<Page scroll={false}>
		{#snippet toolbar()}
			<SelectField
				bind:value={selectedId}
				options={repos.data?.map((r) => ({ value: r.id, label: r.name })) ?? []}
				class="w-64"
			/>
			<span class="min-w-0 truncate text-xs text-muted-foreground">
				{kindLabel} · <span class="selectable font-mono">{selected.repository}</span>
				{#if status?.version}· restic {status.version}{/if}
				{#if selected.sudo}· as root{/if}
			</span>
			<span class="flex-1"></span>
			<IconButton label="Edit repository" onclick={() => (editing = selected)}><Pencil /></IconButton>
			<IconButton label="Remove from Jarvis" onclick={removeRepo}><Trash2 /></IconButton>
			<Button variant="outline" size="sm" onclick={() => (editing = blankRepo())}><Plus /> New</Button>
			<RefreshControl onrefresh={load} {loading} />
		{/snippet}

		{#if statusError}
			<StateView kind="error" error={statusError} onretry={load} />
		{:else if !status}
			<StateView kind="loading" title="Opening the repository…" />
		{:else if status.problem}
			<StateView
				kind="error"
				title="The repository could not be opened"
				message={status.problem}
				onretry={load}
			>
				<Button variant="outline" onclick={() => (editing = selected)}>Edit repository</Button>
			</StateView>
		{:else if !status.initialized}
			<StateView
				kind="empty"
				icon={Archive}
				title="This repository is not initialised yet"
				message="Initialising creates the repository at {selected.repository} and encrypts it with your password."
			>
				<Button onclick={init}>Initialise repository</Button>
			</StateView>
		{:else}
			<div class="mb-2 flex flex-wrap items-center gap-2">
				<Button size="sm" onclick={() => (backupOpen = true)}><Play /> Backup now</Button>
				<Button variant="outline" size="sm" onclick={() => (policyOpen = true)}
					><History /> Retention policy</Button
				>
				<DropdownMenu.Root>
					<DropdownMenu.Trigger>
						{#snippet child({ props })}
							<Button {...props} variant="outline" size="sm">Maintenance</Button>
						{/snippet}
					</DropdownMenu.Trigger>
					<DropdownMenu.Content align="start">
						<DropdownMenu.Item
							onclick={() =>
								run('Check restic repository', () => api.resticMaintenance(selected.id, 'check'))}
							>Integrity check</DropdownMenu.Item
						>
						<DropdownMenu.Item
							onclick={() =>
								run('Unlock restic repository', () => api.resticMaintenance(selected.id, 'unlock'))}
							>Unlock (remove stale locks)</DropdownMenu.Item
						>
						<DropdownMenu.Item
							onclick={() =>
								run('Prune restic repository', () => api.resticMaintenance(selected.id, 'prune'))}
							>Prune unused data</DropdownMenu.Item
						>
					</DropdownMenu.Content>
				</DropdownMenu.Root>
				<Button variant="outline" size="sm" disabled={statsLoading} onclick={loadStats}
					>{statsLoading ? 'Measuring…' : 'Stats'}</Button
				>
				{#if stats}
					<span class="text-xs text-muted-foreground tabular">
						{formatBytes(stats.totalSize)} stored · {formatNumber(stats.snapshotsCount)} snapshots
					</span>
				{/if}
			</div>
			{#if snapshotsError}
				<StateView kind="error" error={snapshotsError} onretry={load} />
			{:else}
				<DataTable
					rows={snapshots}
					{columns}
					rowKey={(s) => s.id}
					selectable
					bind:selected={selection}
					{loading}
					empty="No snapshots yet"
					emptyHint="Run “Backup now” to create the first one."
					onrowdblclick={(s) => (browsing = s)}
					class="flex-1"
				>
					{#snippet cell(s, column)}
						{#if column.key === 'time'}
							{formatDateTime(s.time)}
							<span class="text-xs text-muted-foreground">· {formatRelative(s.time)}</span>
						{:else if column.key === 'tags'}
							{#each s.tags as tag (tag)}<span class="mr-1 rounded bg-muted px-1.5 py-0.5 text-[11px]"
									>{tag}</span
								>{/each}
						{:else}{column.value?.(s)}{/if}
					{/snippet}
					{#snippet actions(s)}
						<Button variant="ghost" size="xs" onclick={() => (browsing = s)}><FolderOpen /> Browse</Button>
						<Button
							variant="ghost"
							size="xs"
							onclick={() => {
								restoring = s;
								restoreTarget = '/tmp/restore';
							}}>Restore</Button
						>
						<IconButton label="Forget snapshot" onclick={() => forget([s])}><Trash2 /></IconButton>
					{/snippet}
					{#snippet bulk(rows)}
						<Button variant="outline" size="xs" onclick={() => forget(rows)}
							><Trash2 /> Forget and prune</Button
						>
					{/snippet}
				</DataTable>
			{/if}
		{/if}
	</Page>
{/if}

<RepoDialog bind:repo={editing} onsaved={saved} />
<JobDialog bind:job />

{#if browsing && selected}
	<SnapshotBrowser repo={selected.id} snapshot={browsing} onclose={() => (browsing = null)} />
{/if}

<Modal
	bind:open={backupOpen}
	title="Backup now"
	description="Creates a new snapshot of the given paths."
	size="lg"
>
	<div class="flex flex-col gap-3">
		<Field label="Paths" hint="One absolute path per line.">
			<Textarea
				bind:value={backupPaths}
				rows={3}
				placeholder={PATHS_PLACEHOLDER}
				class="font-mono text-xs"
				spellcheck="false"
			/>
		</Field>
		<Field label="Tags" hint="Optional, separated by commas.">
			<Input bind:value={backupTags} placeholder="manual, before-upgrade" spellcheck="false" />
		</Field>
		<Field label="Exclude patterns" hint="Optional, one per line, e.g. *.log or /var/www/cache">
			<Textarea bind:value={backupExcludes} rows={2} class="font-mono text-xs" spellcheck="false" />
		</Field>
	</div>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (backupOpen = false)}>Cancel</Button>
		<Button disabled={lines(backupPaths).length === 0} onclick={backup}>Start backup</Button>
	{/snippet}
</Modal>

<Modal
	open={restoring !== null}
	title="Restore snapshot {restoring?.shortId ?? ''}"
	description="The snapshot’s files are written below the target folder, keeping their full original paths."
	size="md"
	onclose={() => (restoring = null)}
>
	<Field
		label="Target folder on the server"
		hint="Use / to restore files to their original locations (existing files are overwritten)."
	>
		<PathInput bind:value={restoreTarget} dirsOnly onsubmit={restore} />
	</Field>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (restoring = null)}>Cancel</Button>
		<Button disabled={!restoreTarget.trim().startsWith('/')} onclick={restore}>Restore</Button>
	{/snippet}
</Modal>

<Modal
	bind:open={policyOpen}
	title="Retention policy"
	description="Keep the newest snapshots by these rules and forget the rest. A value of 0 disables a rule."
	size="md"
>
	<div class="grid grid-cols-2 gap-3">
		<Field label="Keep last"><Input type="number" min="0" bind:value={policy.last} /></Field>
		<Field label="Keep daily"><Input type="number" min="0" bind:value={policy.daily} /></Field>
		<Field label="Keep weekly"><Input type="number" min="0" bind:value={policy.weekly} /></Field>
		<Field label="Keep monthly"><Input type="number" min="0" bind:value={policy.monthly} /></Field>
	</div>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (policyOpen = false)}>Cancel</Button>
		<Button variant="outline" onclick={() => applyPolicy(true)}>Dry run</Button>
		<Button variant="destructive" onclick={() => applyPolicy(false)}>Forget and prune</Button>
	{/snippet}
</Modal>
