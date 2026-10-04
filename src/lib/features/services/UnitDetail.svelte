<!-- Status, journal and unit file of one systemd unit. -->
<script lang="ts">
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Switch } from '$lib/components/ui/switch';
	import FileEditor from '$lib/editor/FileEditor.svelte';
	import { api, type UnitAction, type UnitFile } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { resource } from '$lib/state/resource.svelte';
	import { usePane } from '$lib/workspace/workspace.svelte';

	interface Props {
		unit: string;
		visible: boolean;
		onclose: () => void;
	}

	let { unit, visible, onclose }: Props = $props();

	type View = 'status' | 'logs' | 'file';
	let view = $state<View>('status');
	let follow = $state(false);
	let followJob = $state<Job | null>(null);
	let unitFile = $state<UnitFile | null>(null);
	let acting = $state(false);

	const pane = usePane();
	const status = resource((io) => io.unitStatus(unit));
	const logs = resource((io) => io.unitLogs(unit, 500));

	$effect(() => {
		if (view === 'status') void status.refresh();
		else if (view === 'logs' && !follow) void logs.refresh();
	});

	// Live journal: a hidden streamed job, stopped when leaving the view.
	$effect(() => {
		if (view !== 'logs' || !follow || !visible) return;
		let job: Job | null = null;
		let cancelled = false;
		api.unitLogsFollow(unit).then(
			(id) => {
				job = jobs.get(id);
				if (cancelled) void job.stop();
				else followJob = job;
			},
			(e) => {
				toast.error(e);
				follow = false;
			}
		);
		return () => {
			cancelled = true;
			void job?.stop();
			followJob = null;
		};
	});

	$effect(() => pane?.pushBack('Back to the list', onclose));

	async function act(action: UnitAction): Promise<void> {
		acting = true;
		try {
			await sudo.describe(`${action} ${unit}`, () => api.unitAction(unit, action));
			toast.success(`${unit}: ${action} done`);
		} catch (error) {
			toast.error(error, `Could not ${action} ${unit}`);
		} finally {
			acting = false;
			void status.refresh();
		}
	}

	async function remove(): Promise<void> {
		const ok = await confirm({
			title: `Delete ${unit}?`,
			message:
				'The unit is stopped, disabled and its file removed. Only units created by Jarvis can be deleted here.',
			confirmLabel: 'Delete unit',
			destructive: true
		});
		if (!ok) return;
		try {
			await sudo.describe(`Delete ${unit}`, () => api.unitDelete(unit));
			toast.success(`${unit} deleted`);
			onclose();
		} catch (error) {
			toast.error(error);
		}
	}

	async function readUnit(): Promise<{ content: string; elevated: boolean }> {
		unitFile = await api.unitFile(unit);
		return { content: unitFile.content, elevated: !unitFile.path.startsWith('/home') };
	}

	async function writeUnit(content: string): Promise<boolean> {
		await sudo.describe(`Save ${unit}`, () => api.unitFileSave(unit, content));
		toast.success('Unit saved and systemd reloaded');
		return true;
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex shrink-0 flex-wrap items-center gap-2 px-4 pt-3 pb-2">
		<IconButton label="Back to the list" variant="outline" onclick={onclose}><ArrowLeft /></IconButton>
		<h2 class="selectable mr-auto truncate text-[15px] font-semibold">{unit}</h2>
		<Button variant="outline" size="sm" disabled={acting} onclick={() => act('start')}>Start</Button>
		<Button variant="outline" size="sm" disabled={acting} onclick={() => act('stop')}>Stop</Button>
		<Button variant="outline" size="sm" disabled={acting} onclick={() => act('restart')}>Restart</Button>
		<Button variant="outline" size="sm" disabled={acting} onclick={() => act('reload')}>Reload</Button>
		<Button variant="outline" size="sm" disabled={acting} onclick={() => act('enable')}>Enable</Button>
		<Button variant="outline" size="sm" disabled={acting} onclick={() => act('disable')}>Disable</Button>
		{#if unitFile?.managed}
			<IconButton label="Delete unit" variant="outline" onclick={remove}><Trash2 /></IconButton>
		{/if}
	</header>
	<SubTabs
		class="mx-4"
		bind:value={view}
		items={[
			{ id: 'status', label: 'Status' },
			{ id: 'logs', label: 'Logs' },
			{ id: 'file', label: 'Unit file' }
		]}
	/>
	<div class="flex min-h-0 flex-1 flex-col p-4">
		{#if view === 'status'}
			{#if status.error && !status.loaded}
				<StateView kind="error" error={status.error} onretry={status.refresh} />
			{:else}
				<LogViewer
					source={status.data ?? ''}
					placeholder="Loading…"
					downloadName="{unit}-status.txt"
					class="flex-1"
				/>
			{/if}
		{:else if view === 'logs'}
			{#if !follow && logs.error && !logs.loaded}
				<StateView kind="error" error={logs.error} onretry={logs.refresh} />
			{:else}
				<LogViewer
					source={follow ? followJob : (logs.data ?? '')}
					severity
					live={follow}
					placeholder="No log lines"
					downloadName="{unit}.log"
					class="flex-1"
				>
					{#snippet toolbar()}
						<label class="mr-2 flex items-center gap-1.5 text-xs text-muted-foreground">
							<Switch size="sm" bind:checked={follow} /> Follow
						</label>
						{#if !follow}
							<Button variant="ghost" size="xs" onclick={logs.refresh}>Reload</Button>
						{/if}
					{/snippet}
				</LogViewer>
			{/if}
		{:else}
			<div class="min-h-0 flex-1 overflow-hidden rounded-lg border">
				<FileEditor
					path={unitFile?.path ?? unit}
					language="ini"
					read={readUnit}
					write={writeUnit}
					onclose={() => (view = 'status')}
				/>
			</div>
		{/if}
	</div>
</div>
