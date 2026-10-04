<!-- Backups: one card per template with run, schedule control and its log. -->
<script lang="ts">
	import { open as pickFolder } from '@tauri-apps/plugin-dialog';
	import Archive from '@lucide/svelte/icons/archive';
	import CalendarClock from '@lucide/svelte/icons/calendar-clock';
	import DatabaseIcon from '@lucide/svelte/icons/database';
	import FolderArchive from '@lucide/svelte/icons/folder-archive';
	import Pause from '@lucide/svelte/icons/pause';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Play from '@lucide/svelte/icons/play';
	import Plus from '@lucide/svelte/icons/plus';
	import ScrollText from '@lucide/svelte/icons/scroll-text';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DependencyGuard from '$lib/components/DependencyGuard.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import JobDialog from '$lib/components/JobDialog.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { Button } from '$lib/components/ui/button';
	import { describeCron } from '$lib/cron';
	import { api, toIpcError, type BackupTemplate, type ScheduleInfo, type Tool } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { transfers } from '$lib/services/transfers.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';
	import type { TabProps } from '$lib/workspace/registry';
	import TemplateDialog, { blankTemplate, DESTINATION_LABEL, KIND_LABEL } from './TemplateDialog.svelte';

	let { visible }: TabProps = $props();

	const templates = resource((io) => io.backupTemplates());
	autoLoad(templates, () => visible);

	const busy = new Busy();
	let editing = $state<BackupTemplate | null>(null);
	let job = $state<Job | null>(null);
	let info = $state<{ template: BackupTemplate; data: ScheduleInfo | null } | null>(null);
	/** Tools a run turned out to need; shown with one-click install. */
	let missing = $state<{ template: BackupTemplate; tools: Tool[] } | null>(null);

	function toolsFor(t: BackupTemplate): Tool[] {
		const tools: Tool[] = [];
		if (t.kind !== 'files')
			tools.push(
				t.dbSource === 'container' ? 'docker' : t.kind === 'mysql' ? 'mysqlClient' : 'postgresClient'
			);
		if (t.destination === 's3' || t.destination === 'sftp') tools.push('rclone');
		if (t.destination === 'restic') tools.push('restic');
		return tools;
	}

	function source(t: BackupTemplate): string {
		if (t.kind === 'files') return t.path;
		return `${t.dbName} @ ${t.dbSource === 'container' ? t.dbContainer : `${t.dbHost}:${t.dbPort}`}`;
	}

	function target(t: BackupTemplate): string {
		switch (t.destination) {
			case 'folder':
				return t.folder;
			case 's3':
				return `${t.s3Bucket}${t.s3Prefix ? `/${t.s3Prefix.replace(/^\/+/, '')}` : ''}`;
			case 'sftp':
				return `${t.sftpUser}@${t.sftpHost}:${t.sftpPath}`;
			case 'restic':
				return 'restic repository';
			default:
				return 'this computer';
		}
	}

	async function run(t: BackupTemplate): Promise<void> {
		let localDir: string | null = null;
		if (t.destination === 'download') {
			const dir = await pickFolder({ directory: true, title: `Download “${t.name}” to…` });
			if (typeof dir !== 'string') return;
			localDir = dir;
		}
		await busy.run(
			t.id,
			async () => {
				const id = await sudo.describe(`Run backup “${t.name}”`, () => api.backupRun(t.id, localDir));
				job = jobs.get(id);
				const result = await job.wait();
				if (result.status === 'done') {
					toast.success(`Backup “${t.name}” finished`);
					if (localDir) transfers.panelOpen = true;
				}
			},
			(error) => {
				if (error.is('DEPENDENCY_MISSING')) missing = { template: t, tools: toolsFor(t) };
				else toast.error(error, 'Could not start the backup');
			}
		);
	}

	async function setPaused(t: BackupTemplate, paused: boolean): Promise<void> {
		await busy.run(
			t.id,
			async () => {
				await sudo.describe(paused ? 'Pause the backup schedule' : 'Resume the backup schedule', () =>
					api.backupSetPaused(t.id, paused)
				);
				toast.success(paused ? 'Schedule paused' : 'Schedule resumed');
				await templates.refresh();
			},
			(error) => toast.error(error)
		);
	}

	async function remove(t: BackupTemplate): Promise<void> {
		const ok = await confirm({
			title: `Delete the backup “${t.name}”?`,
			message: t.schedule
				? 'Its schedule, script and stored credentials are removed from the server and from Jarvis. Existing archives are kept.'
				: 'The template and its stored credentials are removed. Existing archives are kept.',
			confirmLabel: 'Delete',
			destructive: true
		});
		if (!ok) return;
		await busy.run(
			t.id,
			async () => {
				await sudo.describe('Remove the backup schedule', () => api.backupTemplateDelete(t.id));
				await templates.refresh();
			},
			(error) => toast.error(error, 'Could not delete the backup')
		);
	}

	async function showLog(t: BackupTemplate): Promise<void> {
		info = { template: t, data: null };
		try {
			const data = await sudo.describe('Read the backup schedule log', () =>
				api.backupScheduleInfo(t.id, 500)
			);
			if (info?.template.id === t.id) info = { template: t, data };
		} catch (e) {
			if (!toIpcError(e).is('CANCELLED')) toast.error(e, 'Could not read the schedule log');
			info = null;
		}
	}

	export const refresh = templates.refresh;
</script>

{#if templates.error && !templates.loaded}
	<StateView kind="error" error={templates.error} onretry={templates.refresh} />
{:else if !templates.loaded}
	<StateView kind="loading" />
{:else if !templates.data?.length}
	<StateView
		kind="empty"
		icon={Archive}
		title="No backups set up yet"
		message="Create a backup of files or a database. Run it when you like, or let the server run it on a schedule."
	>
		<Button onclick={() => (editing = blankTemplate())}><Plus /> New backup</Button>
	</StateView>
{:else}
	<Page title="Backups" subtitle="{templates.data.length} template{templates.data.length === 1 ? '' : 's'}">
		{#snippet toolbar()}
			<Button size="sm" onclick={() => (editing = blankTemplate())}><Plus /> New backup</Button>
			<RefreshControl onrefresh={templates.refresh} loading={templates.loading} />
		{/snippet}
		<div class="grid grid-cols-1 gap-3 xl:grid-cols-2 2xl:grid-cols-3">
			{#each templates.data as t (t.id)}
				{@const working = busy.has(t.id)}
				<article class="flex flex-col gap-3 rounded-xl border bg-card p-4" aria-busy={working}>
					<header class="flex items-start gap-3">
						<span class="grid size-9 shrink-0 place-items-center rounded-lg bg-primary/12 text-primary">
							{#if t.kind === 'files'}<FolderArchive class="size-4.5" />{:else}<DatabaseIcon
									class="size-4.5"
								/>{/if}
						</span>
						<div class="min-w-0 flex-1">
							<h3 class="truncate font-semibold" title={t.name}>{t.name}</h3>
							<p class="selectable truncate font-mono text-xs text-muted-foreground" title={source(t)}>
								{source(t)}
							</p>
						</div>
						<IconButton label="Edit" onclick={() => (editing = t)}><Pencil /></IconButton>
						<IconButton label="Delete" disabled={working} onclick={() => remove(t)}><Trash2 /></IconButton>
					</header>
					<div class="flex flex-wrap gap-1.5 text-xs">
						<span class="rounded-md bg-muted px-2 py-0.5">{KIND_LABEL[t.kind] ?? t.kind}</span>
						<span class="max-w-full truncate rounded-md bg-muted px-2 py-0.5" title={target(t)}
							>{DESTINATION_LABEL[t.destination] ?? t.destination} · {target(t)}</span
						>
						{#if t.schedule}
							<span
								class={t.paused
									? 'rounded-md bg-warning/15 px-2 py-0.5 text-warning'
									: 'rounded-md bg-success/15 px-2 py-0.5 text-success'}
								title={t.schedule}
							>
								<CalendarClock class="mr-0.5 inline size-3 align-[-2px]" />
								{t.paused ? 'Paused' : describeCron(t.schedule) || t.schedule}
							</span>
						{:else}
							<span class="rounded-md bg-muted px-2 py-0.5 text-muted-foreground">Manual</span>
						{/if}
					</div>
					<footer class="mt-auto flex flex-wrap items-center gap-2">
						<Button size="sm" disabled={working} onclick={() => run(t)}
							><Play /> {working ? 'Working…' : 'Run now'}</Button
						>
						{#if t.schedule}
							<Button variant="outline" size="sm" disabled={working} onclick={() => setPaused(t, !t.paused)}>
								{#if t.paused}<Play /> Resume{:else}<Pause /> Pause{/if}
							</Button>
							<Button variant="outline" size="sm" onclick={() => showLog(t)}
								><ScrollText /> Schedule log</Button
							>
						{/if}
					</footer>
				</article>
			{/each}
		</div>
	</Page>
{/if}

<TemplateDialog bind:template={editing} onsaved={() => templates.refresh()} />
<JobDialog bind:job />

{#if info}
	{@const shown = info}
	<Modal
		open
		title="Schedule of “{shown.template.name}”"
		size="xl"
		class="h-[70vh]"
		flush
		onclose={() => (info = null)}
	>
		<div class="flex min-h-0 flex-1 flex-col gap-2 p-4">
			{#if !shown.data}
				<StateView kind="loading" />
			{:else}
				<dl class="grid grid-cols-[8rem_1fr] gap-x-3 gap-y-1 text-sm">
					<dt class="text-muted-foreground">Installed</dt>
					<dd>{shown.data.installed ? 'Yes' : 'No — save the backup again to reinstall its schedule'}</dd>
					<dt class="text-muted-foreground">Crontab entry</dt>
					<dd class="selectable font-mono text-xs break-all">
						{shown.data.cronLine || (shown.template.paused ? 'paused' : 'missing')}
					</dd>
				</dl>
				<LogViewer
					source={shown.data.log}
					severity
					placeholder="The schedule has not run yet"
					downloadName="jarvis-backup-{shown.template.id}.log"
					class="flex-1"
				/>
			{/if}
		</div>
	</Modal>
{/if}

{#if missing}
	{@const need = missing}
	<Modal
		open
		title="This backup needs tools that are not installed"
		size="lg"
		onclose={() => (missing = null)}
	>
		<DependencyGuard tools={need.tools} inline>
			<div class="flex flex-col items-center gap-3 py-6">
				<p class="text-sm">Everything this backup needs is installed.</p>
				<Button
					onclick={() => {
						missing = null;
						void run(need.template);
					}}><Play /> Run now</Button
				>
			</div>
		</DependencyGuard>
	</Modal>
{/if}
