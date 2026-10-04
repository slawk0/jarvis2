<script lang="ts">
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Pencil from '@lucide/svelte/icons/pencil';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import AsyncView from '$lib/components/AsyncView.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import IpLink from '$lib/components/IpLink.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Textarea } from '$lib/components/ui/textarea';
	import { formatBytes, formatDateTime } from '$lib/format';
	import { api, type ContainerAction } from '$lib/ipc';
	import { confirm, prompt } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { poll, resource } from '$lib/state/resource.svelte';
	import { usePane } from '$lib/workspace/workspace.svelte';
	import ContainerEdit from './ContainerEdit.svelte';

	interface Props {
		name: string;
		startEditing: boolean;
		visible: boolean;
		onclose: () => void;
		onlogs: (name: string) => void;
		onshell: (name: string) => void;
	}

	let { name, startEditing, visible, onclose, onlogs, onshell }: Props = $props();

	// The container can be renamed from here, so its name is local state.
	// svelte-ignore state_referenced_locally
	let current = $state(name);
	// svelte-ignore state_referenced_locally
	let editing = $state(startEditing);
	let acting = $state(false);
	let execOpen = $state(false);
	let execCommand = $state('');
	let execOutput = $state('');
	let execRunning = $state(false);
	let networks = $state<string[]>([]);
	const pane = usePane();
	const detail = resource((io) => io.dockerContainerDetail(current));
	const isSecret = (key: string) => /pass|secret|token|key|pwd|credential|api/i.test(key);
	const running = $derived(detail.data?.state === 'running');
	const paused = $derived(detail.data?.state === 'paused');

	$effect(() => {
		void detail.refresh();
	});
	poll(
		() => visible && !editing,
		10_000,
		() => detail.load(true)
	);
	$effect(() => pane?.pushBack('Back to containers', onclose));

	/** Quiet reload after something changed outside this view. */
	export function sync(): void {
		if (!editing) void detail.load(true);
	}

	async function act(action: ContainerAction): Promise<void> {
		if (action === 'remove') {
			const ok = await confirm({
				title: `Remove container “${current}”?`,
				message: 'Data that is not in a volume is lost.',
				confirmLabel: 'Remove',
				destructive: true
			});
			if (!ok) return;
		}
		acting = true;
		try {
			await api.dockerContainerAction([current], action);
			if (action === 'remove') onclose();
			else await detail.refresh();
		} catch (error) {
			toast.error(error);
		} finally {
			acting = false;
		}
	}

	async function rename(): Promise<void> {
		const next = await prompt({
			title: 'Rename container',
			label: 'New name',
			value: current,
			validate: (v) =>
				/^[A-Za-z0-9][A-Za-z0-9_.-]*$/.test(v) ? null : 'Use letters, digits, “_”, “.” and “-”.'
		});
		if (!next || next === current) return;
		try {
			await api.dockerRename(current, next);
			current = next;
			await detail.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	async function setPolicy(policy: string): Promise<void> {
		try {
			await api.dockerSetRestartPolicy(current, policy);
			toast.success('Restart policy updated');
			await detail.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	async function joinNetwork(): Promise<void> {
		try {
			networks = (await api.dockerNetworks())
				.map((n) => n.name)
				.filter((n) => !detail.data?.networks.some((a) => a.name === n));
		} catch (error) {
			toast.error(error);
		}
	}

	async function connect(network: string, on: boolean): Promise<void> {
		try {
			await api.dockerNetworkConnect(current, network, on);
			networks = [];
			await detail.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	async function runExec(): Promise<void> {
		execRunning = true;
		try {
			execOutput = await api.dockerExec(current, execCommand);
		} catch (error) {
			toast.error(error);
		} finally {
			execRunning = false;
		}
	}
</script>

{#snippet section(title: string)}
	<h3 class="mt-5 mb-1.5 text-xs font-medium tracking-wide text-muted-foreground uppercase">{title}</h3>
{/snippet}

<div class="flex h-full min-h-0 flex-col">
	<header class="flex shrink-0 flex-wrap items-center gap-2 px-4 pt-3 pb-2">
		<IconButton label="Back to containers" variant="outline" onclick={onclose}><ArrowLeft /></IconButton>
		<h2 class="selectable truncate text-[15px] font-semibold">{current}</h2>
		<IconButton label="Rename" onclick={rename}><Pencil /></IconButton>
		{#if detail.data}
			<span class="rounded bg-muted px-1.5 py-0.5 text-xs">{detail.data.state}</span>
		{/if}
		<span class="flex-1"></span>
		<Button variant="outline" size="sm" disabled={acting || running} onclick={() => act('start')}
			>Start</Button
		>
		<Button variant="outline" size="sm" disabled={acting || !(running || paused)} onclick={() => act('stop')}
			>Stop</Button
		>
		<Button variant="outline" size="sm" disabled={acting} onclick={() => act('restart')}>Restart</Button>
		<Button
			variant="outline"
			size="sm"
			disabled={acting || !(running || paused)}
			onclick={() => act(paused ? 'unpause' : 'pause')}>{paused ? 'Unpause' : 'Pause'}</Button
		>
		<Button variant="outline" size="sm" disabled={acting || !running} onclick={() => act('kill')}>Kill</Button
		>
		<Button variant="outline" size="sm" onclick={() => onlogs(current)}>Logs</Button>
		<Button variant="outline" size="sm" disabled={!running} onclick={() => onshell(current)}>Shell</Button>
		<Button
			variant="outline"
			size="sm"
			disabled={!running}
			onclick={() => {
				execOutput = '';
				execOpen = true;
			}}>Exec</Button
		>
		<Button size="sm" onclick={() => (editing = true)}>Edit</Button>
		<Button variant="destructive" size="sm" disabled={acting} onclick={() => act('remove')}>Remove</Button>
	</header>

	<div class="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
		<AsyncView resource={detail}>
			{#snippet children(d)}
				{#if d.composeProject}
					<p
						class="flex items-center gap-2 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-xs"
					>
						<TriangleAlert class="size-4 shrink-0 text-warning" />
						Managed by Docker Compose project “{d.composeProject}”. Changes made here are lost the next time
						the stack is recreated; edit the compose file instead.
					</p>
				{/if}
				<dl class="selectable mt-3 grid grid-cols-[10rem_1fr] gap-x-4 gap-y-1.5 text-sm">
					<dt class="text-muted-foreground">Image</dt>
					<dd class="font-mono text-xs">{d.image}</dd>
					<dt class="text-muted-foreground">Created</dt>
					<dd>{formatDateTime(d.created)}</dd>
					<dt class="text-muted-foreground">Started</dt>
					<dd>{d.started.startsWith('0001') ? '—' : formatDateTime(d.started)}</dd>
					<dt class="text-muted-foreground">Command</dt>
					<dd class="font-mono text-xs break-all">{d.commandLine || '—'}</dd>
					<dt class="text-muted-foreground">Entrypoint</dt>
					<dd class="font-mono text-xs break-all">{d.entrypoint.join(' ') || '—'}</dd>
					<dt class="text-muted-foreground">Restart policy</dt>
					<dd>
						<SelectField
							size="sm"
							class="w-44"
							value={d.restartPolicy.split(':')[0]}
							options={['no', 'always', 'unless-stopped', 'on-failure']}
							onchange={setPolicy}
						/>
					</dd>
					{#if d.memory || d.nanoCpus}
						<dt class="text-muted-foreground">Limits</dt>
						<dd>
							{d.memory ? `${formatBytes(d.memory)} memory` : ''}{d.memory && d.nanoCpus
								? ' · '
								: ''}{d.nanoCpus ? `${d.nanoCpus / 1e9} CPUs` : ''}
						</dd>
					{/if}
				</dl>

				{@render section('Port mappings')}
				{#if d.ports.length}
					<ul class="selectable font-mono text-xs">
						{#each d.ports as p (p.hostIp + p.hostPort + p.containerPort + p.protocol)}
							<li>{p.hostIp || '0.0.0.0'}:{p.hostPort} → {p.containerPort}/{p.protocol}</li>
						{/each}
					</ul>
				{:else}<p class="text-xs text-muted-foreground">No published ports.</p>{/if}

				{@render section('Networks')}
				<table class="w-full max-w-3xl text-sm">
					<tbody>
						{#each d.networks as n (n.name)}
							<tr class="border-b last:border-b-0">
								<td class="py-1.5 font-medium">{n.name}</td>
								<td class="py-1.5 text-xs"><IpLink ip={n.ip} /></td>
								<td class="py-1.5 text-xs text-muted-foreground">gateway <IpLink ip={n.gateway} /></td>
								<td class="selectable py-1.5 font-mono text-xs text-muted-foreground">{n.mac}</td>
								<td class="py-1 text-right"
									><Button variant="ghost" size="xs" onclick={() => connect(n.name, false)}>Leave</Button></td
								>
							</tr>
						{/each}
					</tbody>
				</table>
				<div class="mt-1.5 flex items-center gap-2">
					{#if networks.length}
						<SelectField
							size="sm"
							class="w-56"
							value=""
							placeholder="Choose a network…"
							options={networks}
							onchange={(n) => connect(n, true)}
						/>
						<Button variant="ghost" size="xs" onclick={() => (networks = [])}>Cancel</Button>
					{:else}
						<Button variant="outline" size="xs" onclick={joinNetwork}>Join a network…</Button>
					{/if}
				</div>

				{@render section('Mounts')}
				{#if d.mounts.length}
					<ul class="selectable flex flex-col gap-1 font-mono text-xs">
						{#each d.mounts as m (m.target)}
							<li>
								<span class="text-muted-foreground">{m.kind}</span>
								{m.source} → {m.target}{m.readOnly ? ' (read-only)' : ''}
							</li>
						{/each}
					</ul>
				{:else}<p class="text-xs text-muted-foreground">No mounts.</p>{/if}

				{@render section('Environment')}
				{#if d.env.length}
					<ul class="selectable font-mono text-xs">
						{#each d.env as [key, value] (key)}
							<li class="break-all">
								<span class="text-primary">{key}</span>={isSecret(key) ? '••••••••' : value}
							</li>
						{/each}
					</ul>
				{:else}<p class="text-xs text-muted-foreground">No variables.</p>{/if}

				{@render section('Labels')}
				{#if d.labels.length}
					<ul class="selectable font-mono text-xs">
						{#each d.labels as [key, value] (key)}
							<li class="break-all"><span class="text-muted-foreground">{key}</span>={value}</li>
						{/each}
					</ul>
				{:else}<p class="text-xs text-muted-foreground">No labels.</p>{/if}

				{#if editing}
					<ContainerEdit
						detail={d}
						onclose={() => (editing = false)}
						ondone={(newName) => {
							editing = false;
							current = newName;
							void detail.refresh();
						}}
					/>
				{/if}
			{/snippet}
		</AsyncView>
	</div>
</div>

<Modal bind:open={execOpen} title="Run a command in {current}" size="lg">
	<form
		class="flex gap-2"
		onsubmit={(e) => {
			e.preventDefault();
			void runExec();
		}}
	>
		<Textarea
			bind:value={execCommand}
			rows={2}
			class="font-mono text-xs"
			placeholder="ls -la /"
			spellcheck="false"
		/>
		<Button type="submit" disabled={execRunning || !execCommand.trim()}
			>{execRunning ? 'Running…' : 'Run'}</Button
		>
	</form>
	{#if execOutput}
		<pre
			class="selectable mt-3 max-h-80 overflow-auto rounded-lg border bg-terminal p-3 text-xs">{execOutput}</pre>
	{/if}
</Modal>
