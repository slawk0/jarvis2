<script lang="ts">
	import FileText from '@lucide/svelte/icons/file-text';
	import Play from '@lucide/svelte/icons/play';
	import Plus from '@lucide/svelte/icons/plus';
	import DataTable, { type Column, type Sort } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { Input } from '$lib/components/ui/input';
	import { formatDateTime, formatRelative } from '$lib/format';
	import { api, type TimerUnit, type UnitAction } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';

	let { visible }: TabProps = $props();

	const timers = resource((io) => io.timersList());
	const busy = new Busy();
	let search = $state('');
	let sort = $state<Sort | null>({ key: 'next', dir: 'asc' });
	let inspect = $state<{ unit: string; text: string } | null>(null);
	let createOpen = $state(false);
	let submitted = $state(false);
	let creating = $state(false);
	let form = $state({
		name: '',
		description: '',
		onCalendar: 'daily',
		command: '',
		user: '',
		persistent: true
	});

	autoLoad(timers, () => visible, 30_000);

	const columns: Column<TimerUnit>[] = [
		{ key: 'unit', label: 'Timer', value: (t) => t.unit, class: 'max-w-0 w-full' },
		// Timers without a next run sort last.
		{ key: 'next', label: 'Next run', value: (t) => t.next ?? Number.MAX_SAFE_INTEGER, class: 'w-52' },
		{ key: 'last', label: 'Last run', value: (t) => t.last ?? 0, class: 'w-52' },
		{ key: 'enabled', label: 'On boot', value: (t) => t.enabled, class: 'w-24' },
		{ key: 'active', label: 'State', value: (t) => t.active, class: 'w-24' }
	];

	async function run(timer: TimerUnit, label: string, action: () => Promise<unknown>): Promise<void> {
		const ok = await busy.run(
			timer.unit,
			() => sudo.describe(`${label} ${timer.unit}`, action),
			(e) => toast.error(e)
		);
		if (ok) toast.success(`${timer.unit}: ${label.toLowerCase()} done`);
		await timers.load(true);
	}

	const act = (timer: TimerUnit, action: UnitAction) =>
		run(timer, action, () => api.unitAction(timer.unit, action));
	const runNow = (timer: TimerUnit) => run(timer, 'Run now', () => api.timerRunNow(timer.unit));

	async function show(timer: TimerUnit): Promise<void> {
		try {
			inspect = { unit: timer.unit, text: await api.timerInspect(timer.unit) };
		} catch (error) {
			toast.error(error);
		}
	}

	async function remove(timer: TimerUnit): Promise<void> {
		const ok = await confirm({
			title: `Delete ${timer.unit}?`,
			message:
				'The timer and its service are stopped, disabled and removed. Only timers created by Jarvis can be deleted here.',
			confirmLabel: 'Delete timer',
			destructive: true
		});
		if (ok) await run(timer, 'Delete', () => api.unitDelete(timer.unit));
	}

	const errors = $derived({
		name: !/^[A-Za-z0-9_.@-]+$/.test(form.name) ? 'Use letters, digits, “-”, “_” or “.”.' : null,
		onCalendar: !form.onCalendar.trim() ? 'Enter a schedule.' : null,
		command: !form.command.trim().startsWith('/') ? 'Enter a command with an absolute path.' : null
	});

	async function create(): Promise<void> {
		submitted = true;
		if (errors.name || errors.onCalendar || errors.command) return;
		creating = true;
		try {
			await sudo.describe(`Create timer ${form.name}`, () => api.timerCreate($state.snapshot(form)));
			toast.success(`Timer ${form.name} created`);
			createOpen = false;
			await timers.refresh();
		} catch (error) {
			toast.error(error, 'Could not create the timer');
		} finally {
			creating = false;
		}
	}

	export const refresh = () => timers.refresh();
</script>

<Page scroll={false}>
	{#snippet toolbar()}
		<SearchInput bind:value={search} placeholder="Filter timers…" class="w-64" />
		<span class="text-xs text-muted-foreground tabular">{timers.data?.length ?? 0} timers</span>
		<span class="flex-1"></span>
		<Button
			size="sm"
			onclick={() => {
				submitted = false;
				createOpen = true;
			}}><Plus /> Create timer</Button
		>
		<RefreshControl onrefresh={timers.refresh} loading={timers.loading} />
	{/snippet}
	<DataTable
		rows={timers.data ?? []}
		{columns}
		rowKey={(t) => t.unit}
		{search}
		bind:sort
		busy={busy.keys}
		loading={timers.loading}
		error={timers.error}
		onretry={timers.refresh}
		empty="No timers"
		rowHeight={44}
		class="flex-1"
	>
		{#snippet cell(t, column)}
			{#if column.key === 'unit'}
				<div class="min-w-0">
					<p class="truncate font-medium">{t.unit.replace(/\.timer$/, '')}</p>
					<p class="truncate text-xs text-muted-foreground">{t.description || t.activates}</p>
				</div>
			{:else if column.key === 'next' || column.key === 'last'}
				{@const stamp = column.key === 'next' ? t.next : t.last}
				{#if stamp}
					<span class="tabular">{formatDateTime(stamp)}</span>
					<span class="text-xs text-muted-foreground">· {formatRelative(stamp)}</span>
				{:else}
					<span class="text-muted-foreground">{column.key === 'next' ? 'not scheduled' : 'never'}</span>
				{/if}
			{:else if column.key === 'enabled'}
				<span class={t.enabled === 'enabled' ? 'text-success' : 'text-muted-foreground'}
					>{t.enabled || '—'}</span
				>
			{:else}
				<span class="flex items-center gap-2">
					<span
						class={cn('size-2 rounded-full', t.active === 'active' ? 'bg-success' : 'bg-muted-foreground/40')}
					></span>
					{t.active}
				</span>
			{/if}
		{/snippet}
		{#snippet actions(t)}
			<IconButton label="Run the service now" onclick={() => runNow(t)}><Play /></IconButton>
			<IconButton label="Inspect" onclick={() => show(t)}><FileText /></IconButton>
		{/snippet}
		{#snippet menu(t)}
			<ContextMenu.Item onclick={() => runNow(t)}>Run now</ContextMenu.Item>
			<ContextMenu.Item onclick={() => act(t, 'start')}>Start timer</ContextMenu.Item>
			<ContextMenu.Item onclick={() => act(t, 'stop')}>Stop timer</ContextMenu.Item>
			<ContextMenu.Separator />
			<ContextMenu.Item onclick={() => act(t, 'enable')}>Enable on boot</ContextMenu.Item>
			<ContextMenu.Item onclick={() => act(t, 'disable')}>Disable on boot</ContextMenu.Item>
			<ContextMenu.Separator />
			<ContextMenu.Item onclick={() => show(t)}>Inspect</ContextMenu.Item>
			<ContextMenu.Item variant="destructive" onclick={() => remove(t)}>Delete…</ContextMenu.Item>
		{/snippet}
	</DataTable>
</Page>

{#if inspect}
	<Modal open title={inspect.unit} size="xl" class="h-[70vh]" flush onclose={() => (inspect = null)}>
		<LogViewer source={inspect.text} downloadName="{inspect.unit}.txt" class="flex-1" />
	</Modal>
{/if}

<Modal
	bind:open={createOpen}
	title="Create timer"
	description="Creates a timer and the service it runs in /etc/systemd/system."
	size="lg"
>
	<form
		id="timer-form"
		class="grid grid-cols-2 gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void create();
		}}
	>
		<Field label="Name" required error={submitted ? errors.name : null}
			><Input bind:value={form.name} placeholder="nightly-cleanup" spellcheck="false" autofocus /></Field
		>
		<Field label="Description"><Input bind:value={form.description} /></Field>
		<Field
			label="Schedule (OnCalendar)"
			required
			error={submitted ? errors.onCalendar : null}
			hint="For example: hourly, daily, Mon *-*-* 03:00:00"
			class="col-span-2"
		>
			<Input bind:value={form.onCalendar} class="font-mono text-xs" spellcheck="false" />
		</Field>
		<Field label="Command" required error={submitted ? errors.command : null} class="col-span-2">
			<Input
				bind:value={form.command}
				placeholder="/usr/local/bin/cleanup.sh"
				class="font-mono text-xs"
				spellcheck="false"
			/>
		</Field>
		<Field label="Run as user" hint="Empty runs as root."
			><Input bind:value={form.user} spellcheck="false" /></Field
		>
		<label class="flex items-center gap-2 self-end pb-2 text-sm"
			><Checkbox bind:checked={form.persistent} /> Catch up after downtime</label
		>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
		<Button type="submit" form="timer-form" disabled={creating}
			>{creating ? 'Creating…' : 'Create timer'}</Button
		>
	{/snippet}
</Modal>
