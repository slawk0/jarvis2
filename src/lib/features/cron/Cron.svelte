<script lang="ts">
	import Info from '@lucide/svelte/icons/info';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import AsyncView from '$lib/components/AsyncView.svelte';
	import CronInput from '$lib/components/CronInput.svelte';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import { describeCron, isValidCron } from '$lib/cron';
	import { api, type CronJob } from '$lib/ipc';
	import { app } from '$lib/services/app.svelte';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { Busy, resource } from '$lib/state/resource.svelte';
	import type { TabProps } from '$lib/workspace/registry';
	import { workspace } from '$lib/workspace/workspace.svelte';

	let { visible }: TabProps = $props();

	type View = 'user' | 'root' | 'system';
	let view = $state<View>('user');
	const user = resource((io) => io.cronList());
	const root = resource((io) => io.cronRoot());
	const system = resource((io) => io.cronSystemFiles());
	const busy = new Busy();

	let formOpen = $state(false);
	let editing = $state<CronJob | null>(null);
	let schedule = $state('0 3 * * *');
	let command = $state('');
	let saving = $state(false);

	const loaded = new Set<View>();
	$effect(() => {
		if (!visible || loaded.has(view)) return;
		loaded.add(view);
		void { user, root, system }[view].refresh();
	});

	const columns: Column<CronJob>[] = [
		{ key: 'enabled', label: 'On', sortable: false, class: 'w-14' },
		{ key: 'schedule', label: 'Schedule', value: (j) => j.schedule, class: 'w-72' },
		{ key: 'command', label: 'Command', value: (j) => j.command, mono: true, class: 'max-w-0 w-full' }
	];
	const readOnlyColumns = columns.slice(1);
	const key = (j: CronJob) => `${j.line}:${j.schedule}:${j.command}`;
	const ref = (j: CronJob) => ({ line: j.line, schedule: j.schedule, command: j.command });

	function openForm(job: CronJob | null): void {
		editing = job;
		schedule = job?.schedule ?? '0 3 * * *';
		command = job?.command ?? '';
		formOpen = true;
	}

	async function save(): Promise<void> {
		if (!isValidCron(schedule) || !command.trim()) return;
		saving = true;
		try {
			if (editing) await api.cronUpdate(ref(editing), schedule.trim(), command.trim());
			else await api.cronAdd(schedule.trim(), command.trim());
			toast.success(editing ? 'Cron job updated' : 'Cron job added');
			formOpen = false;
			await user.refresh();
		} catch (error) {
			toast.error(error, 'Could not save the cron job');
		} finally {
			saving = false;
		}
	}

	async function toggle(job: CronJob, enabled: boolean): Promise<void> {
		await busy.run(
			key(job),
			() => api.cronSetEnabled(ref(job), enabled),
			(e) => toast.error(e)
		);
		await user.refresh();
	}

	async function remove(job: CronJob): Promise<void> {
		const ok = await confirm({
			title: 'Delete this cron job?',
			detail: `${job.schedule} ${job.command}`,
			confirmLabel: 'Delete',
			destructive: true
		});
		if (!ok) return;
		await busy.run(
			key(job),
			() => api.cronDelete(ref(job)),
			(e) => toast.error(e)
		);
		await user.refresh();
	}

	export function refresh(): void {
		void { user, root, system }[view].refresh();
	}
</script>

{#snippet scheduleCell(job: CronJob)}
	<span class="font-mono text-xs">{job.schedule}</span>
	<span class="ml-2 text-xs text-muted-foreground">{describeCron(job.schedule)}</span>
{/snippet}

<Page scroll={false}>
	{#snippet toolbar()}
		{#if view === 'user'}
			<Button size="sm" onclick={() => openForm(null)}><Plus /> Add cron job</Button>
		{/if}
		<span class="flex-1"></span>
		<RefreshControl onrefresh={refresh} loading={user.loading || root.loading || system.loading} />
	{/snippet}
	{#snippet header()}
		<SubTabs
			bind:value={view}
			items={[
				{ id: 'user', label: `${app.session?.user ?? 'User'}’s crontab`, count: user.data?.jobs.length },
				{ id: 'root', label: 'root’s crontab' },
				{ id: 'system', label: '/etc/cron.d' }
			]}
		/>
	{/snippet}

	{#if view === 'user'}
		<DataTable
			rows={user.data?.jobs ?? []}
			{columns}
			rowKey={key}
			busy={busy.keys}
			loading={user.loading}
			error={user.error}
			onretry={user.refresh}
			empty="No cron jobs"
			emptyHint="Lines that are not jobs (variables, comments) are kept untouched."
			class="flex-1"
		>
			{#snippet cell(job, column)}
				{#if column.key === 'enabled'}
					<Switch
						size="sm"
						checked={job.enabled}
						disabled={job.managed}
						onCheckedChange={(v) => toggle(job, v)}
						aria-label="Enabled"
					/>
				{:else if column.key === 'schedule'}
					{@render scheduleCell(job)}
				{:else}
					<span class={job.enabled ? '' : 'text-muted-foreground line-through'}>{job.command}</span>
				{/if}
			{/snippet}
			{#snippet actions(job)}
				{#if job.managed}
					<span class="px-2 text-xs text-muted-foreground">managed by Backups</span>
				{:else}
					<IconButton label="Edit" onclick={() => openForm(job)}><Pencil /></IconButton>
					<IconButton label="Delete" onclick={() => remove(job)}><Trash2 /></IconButton>
				{/if}
			{/snippet}
		</DataTable>
	{:else if view === 'root'}
		<p class="mb-2 flex items-center gap-2 text-xs text-muted-foreground">
			<Info class="size-3.5" />
			Read-only. Backup schedules created by Jarvis live here and are managed in
			<button type="button" class="text-primary hover:underline" onclick={() => workspace.openTab('backups')}
				>Backups</button
			>.
		</p>
		<DataTable
			rows={root.data?.jobs ?? []}
			columns={readOnlyColumns}
			rowKey={key}
			loading={root.loading}
			error={root.error}
			onretry={root.refresh}
			empty="root has no cron jobs"
			class="flex-1"
		>
			{#snippet cell(job, column)}
				{#if column.key === 'schedule'}
					{@render scheduleCell(job)}
				{:else}
					<span class={job.enabled ? '' : 'text-muted-foreground line-through'}>{job.command}</span>
				{/if}
			{/snippet}
		</DataTable>
	{:else}
		<AsyncView resource={system}>
			{#snippet children(files)}
				<div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
					{#each files as file (file.path)}
						<section>
							<h3 class="selectable mb-1 font-mono text-xs font-medium">{file.path}</h3>
							<LogViewer source={file.content} controls={false} class="h-40" />
						</section>
					{:else}
						<p class="text-sm text-muted-foreground">There are no files in /etc/cron.d.</p>
					{/each}
				</div>
			{/snippet}
		</AsyncView>
	{/if}
</Page>

<Modal bind:open={formOpen} title={editing ? 'Edit cron job' : 'Add cron job'} size="lg">
	<form
		id="cron-form"
		class="flex flex-col gap-4"
		onsubmit={(e) => {
			e.preventDefault();
			void save();
		}}
	>
		<Field label="Schedule" required><CronInput bind:value={schedule} /></Field>
		<Field label="Command" required hint="Runs as {app.session?.user} with sh.">
			<Input
				bind:value={command}
				class="font-mono text-xs"
				spellcheck="false"
				placeholder="/usr/local/bin/task.sh >> /var/log/task.log 2>&1"
			/>
		</Field>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (formOpen = false)}>Cancel</Button>
		<Button type="submit" form="cron-form" disabled={saving || !isValidCron(schedule) || !command.trim()}
			>{saving ? 'Saving…' : 'Save'}</Button
		>
	{/snippet}
</Modal>
