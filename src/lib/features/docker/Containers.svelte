<script lang="ts">
	import Braces from '@lucide/svelte/icons/braces';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Play from '@lucide/svelte/icons/play';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import ScrollText from '@lucide/svelte/icons/scroll-text';
	import Square from '@lucide/svelte/icons/square';
	import SquareTerminal from '@lucide/svelte/icons/square-terminal';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column, type Sort } from '$lib/components/DataTable.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { api, type Container, type ContainerAction } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';
	import { workspace } from '$lib/workspace/workspace.svelte';
	import ContainerDetail from './ContainerDetail.svelte';
	import ContainerLogs from './ContainerLogs.svelte';

	interface Props {
		visible: boolean;
		onchanged: () => void;
	}

	let { visible, onchanged }: Props = $props();

	const containers = resource((io) => io.dockerContainers());
	const busy = new Busy();
	let search = $state('');
	let sort = $state<Sort | null>({ key: 'name', dir: 'asc' });
	let selected = $state(new Set<string>());
	let sub = $state<{ kind: 'detail' | 'logs'; name: string; edit?: boolean } | null>(null);
	let inspect = $state<{ name: string; json: string } | null>(null);
	let shellFor = $state<string | null>(null);
	let detailView = $state<ContainerDetail>();
	let shell = $state('sh');

	autoLoad(containers, () => visible && !sub, 10_000);

	const columns: Column<Container>[] = [
		{ key: 'name', label: 'Name', value: (c) => c.name, class: 'max-w-64' },
		{ key: 'image', label: 'Image', value: (c) => c.image, mono: true, class: 'max-w-64' },
		{ key: 'status', label: 'Status', value: (c) => `${c.state} ${c.status}`, class: 'w-56 max-w-56' },
		{ key: 'ports', label: 'Ports', value: (c) => c.ports, mono: true, class: 'max-w-0 w-full' },
		{ key: 'created', label: 'Created', value: (c) => c.created, class: 'w-44 max-w-44 tabular' }
	];
	const TONE: Record<string, string> = {
		running: 'bg-success',
		paused: 'bg-warning',
		restarting: 'bg-warning',
		dead: 'bg-destructive'
	};
	const PAST: Record<ContainerAction, string> = {
		start: 'started',
		stop: 'stopped',
		restart: 'restarted',
		kill: 'killed',
		pause: 'paused',
		unpause: 'resumed',
		remove: 'removed'
	};

	async function act(names: string[], action: ContainerAction): Promise<void> {
		if (action === 'remove') {
			const ok = await confirm({
				title: names.length === 1 ? `Remove container “${names[0]}”?` : `Remove ${names.length} containers?`,
				message: 'Running containers are stopped first. Data that is not in a volume is lost.',
				detail: names.length > 1 ? names.join('\n') : undefined,
				confirmLabel: 'Remove',
				destructive: true
			});
			if (!ok) return;
		}
		const ok = await busy.run(
			names,
			() => api.dockerContainerAction(names, action),
			(e) => toast.error(e)
		);
		if (ok)
			toast.success(
				names.length === 1 ? `${names[0]} ${PAST[action]}` : `${names.length} containers ${PAST[action]}`
			);
		selected = new Set();
		await containers.load(true);
		onchanged();
	}

	async function showInspect(name: string): Promise<void> {
		try {
			inspect = { name, json: await api.dockerInspect('container', name) };
		} catch (error) {
			toast.error(error);
		}
	}

	function openShell(): void {
		if (!shellFor) return;
		workspace.request('terminal', { kind: 'container', container: shellFor, shell }, { newPane: true });
		shellFor = null;
	}

	export const refresh = () => containers.refresh();
	/** Quiet reload after something changed outside this view. */
	export function sync(): void {
		void containers.load(true);
		detailView?.sync();
	}
	export function reset(): void {
		sub = null;
	}
</script>

{#if sub?.kind === 'detail'}
	{#key sub.name}
		<ContainerDetail
			bind:this={detailView}
			name={sub.name}
			startEditing={sub.edit ?? false}
			{visible}
			onclose={() => {
				sub = null;
				void containers.load(true);
				onchanged();
			}}
			onlogs={(name) => (sub = { kind: 'logs', name })}
			onshell={(name) => (shellFor = name)}
		/>
	{/key}
{:else if sub?.kind === 'logs'}
	{#key sub.name}
		<ContainerLogs name={sub.name} {visible} onclose={() => (sub = null)} />
	{/key}
{:else}
	<Page scroll={false}>
		{#snippet toolbar()}
			<SearchInput bind:value={search} placeholder="Search containers…" class="w-64" />
			<span class="flex-1"></span>
			<RefreshControl onrefresh={containers.refresh} loading={containers.loading} />
		{/snippet}
		<DataTable
			rows={containers.data ?? []}
			{columns}
			rowKey={(c) => c.name}
			{search}
			bind:sort
			selectable
			bind:selected
			busy={busy.keys}
			loading={containers.loading}
			error={containers.error}
			onretry={containers.refresh}
			empty="No containers"
			onrowdblclick={(c) => (sub = { kind: 'detail', name: c.name })}
			class="flex-1"
		>
			{#snippet cell(c, column)}
				{#if column.key === 'name'}
					<button
						type="button"
						class="flex items-center gap-2 truncate text-left font-medium hover:text-primary"
						onclick={() => (sub = { kind: 'detail', name: c.name })}
					>
						<span class={cn('size-2 shrink-0 rounded-full', TONE[c.state] ?? 'bg-muted-foreground/40')}
						></span>
						<span class="truncate">{c.name}</span>
						{#if c.composeProject}<span
								class="shrink-0 rounded bg-muted px-1 text-[10px] font-normal text-muted-foreground"
								>{c.composeProject}</span
							>{/if}
					</button>
				{:else if column.key === 'status'}
					{c.status}
				{:else}
					{column.value?.(c)}
				{/if}
			{/snippet}
			{#snippet actions(c)}
				{#if c.state === 'running' || c.state === 'restarting' || c.state === 'paused'}
					<IconButton label="Restart" onclick={() => act([c.name], 'restart')}><RotateCw /></IconButton>
					<IconButton label="Stop" onclick={() => act([c.name], 'stop')}><Square /></IconButton>
				{:else}
					<IconButton label="Start" onclick={() => act([c.name], 'start')}><Play /></IconButton>
				{/if}
				<IconButton label="Logs" onclick={() => (sub = { kind: 'logs', name: c.name })}
					><ScrollText /></IconButton
				>
				<IconButton label="Shell" disabled={c.state !== 'running'} onclick={() => (shellFor = c.name)}
					><SquareTerminal /></IconButton
				>
				<IconButton label="Inspect" onclick={() => showInspect(c.name)}><Braces /></IconButton>
				<IconButton label="Edit" onclick={() => (sub = { kind: 'detail', name: c.name, edit: true })}
					><Pencil /></IconButton
				>
				<IconButton label="Remove" onclick={() => act([c.name], 'remove')}><Trash2 /></IconButton>
			{/snippet}
			{#snippet bulk(rows)}
				{@const names = rows.map((r) => r.name)}
				<Button variant="outline" size="xs" onclick={() => act(names, 'start')}>Start</Button>
				<Button variant="outline" size="xs" onclick={() => act(names, 'stop')}>Stop</Button>
				<Button variant="outline" size="xs" onclick={() => act(names, 'restart')}>Restart</Button>
				<Button variant="destructive" size="xs" onclick={() => act(names, 'remove')}>Remove</Button>
			{/snippet}
			{#snippet menu(c)}
				<ContextMenu.Item onclick={() => (sub = { kind: 'detail', name: c.name })}>Details</ContextMenu.Item>
				<ContextMenu.Item onclick={() => (sub = { kind: 'logs', name: c.name })}>Logs</ContextMenu.Item>
				<ContextMenu.Item disabled={c.state !== 'running'} onclick={() => (shellFor = c.name)}
					>Shell…</ContextMenu.Item
				>
				<ContextMenu.Item onclick={() => showInspect(c.name)}>Inspect</ContextMenu.Item>
				<ContextMenu.Separator />
				<ContextMenu.Item onclick={() => act([c.name], 'start')}>Start</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act([c.name], 'stop')}>Stop</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act([c.name], 'restart')}>Restart</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act([c.name], c.state === 'paused' ? 'unpause' : 'pause')}
					>{c.state === 'paused' ? 'Unpause' : 'Pause'}</ContextMenu.Item
				>
				<ContextMenu.Item onclick={() => act([c.name], 'kill')}>Kill</ContextMenu.Item>
				<ContextMenu.Separator />
				<ContextMenu.Item variant="destructive" onclick={() => act([c.name], 'remove')}
					>Remove…</ContextMenu.Item
				>
			{/snippet}
		</DataTable>
	</Page>
{/if}

{#if inspect}
	<Modal
		open
		title="Inspect · {inspect.name}"
		size="xl"
		class="h-[75vh]"
		flush
		onclose={() => (inspect = null)}
	>
		<LogViewer source={inspect.json} downloadName="{inspect.name}.json" class="flex-1" />
	</Modal>
{/if}

{#if shellFor}
	<Modal
		open
		title="Open a shell in {shellFor}"
		description="Opens a terminal attached to the container."
		size="sm"
		onclose={() => (shellFor = null)}
	>
		<SelectField bind:value={shell} options={['sh', 'bash', 'ash', 'zsh']} />
		{#snippet footer()}
			<Button variant="outline" onclick={() => (shellFor = null)}>Cancel</Button>
			<Button onclick={openShell}>Open shell</Button>
		{/snippet}
	</Modal>
{/if}
