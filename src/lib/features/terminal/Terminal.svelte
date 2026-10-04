<script lang="ts">
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Bookmark from '@lucide/svelte/icons/bookmark';
	import Box from '@lucide/svelte/icons/box';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import FilePen from '@lucide/svelte/icons/file-pen';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Play from '@lucide/svelte/icons/play';
	import Plus from '@lucide/svelte/icons/plus';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import Settings from '@lucide/svelte/icons/settings';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import X from '@lucide/svelte/icons/x';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import FileEditor from '$lib/editor/FileEditor.svelte';
	import { api, type TerminalTarget } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { loadDoc, saveDoc, type SavedCommand } from '$lib/services/profile-data';
	import { toast } from '$lib/services/toast.svelte';
	import { cn, matches, uid } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';
	import { usePane, workspace } from '$lib/workspace/workspace.svelte';
	import TerminalView from './TerminalView.svelte';

	let { visible }: TabProps = $props();

	interface Session {
		id: string;
		target: TerminalTarget;
		running: boolean;
	}

	const DEFAULT_COMMANDS: Omit<SavedCommand, 'id'>[] = [
		{ name: 'Disk usage', command: 'df -h' },
		{ name: 'Memory', command: 'free -m' },
		{ name: 'Containers', command: 'docker ps' },
		{ name: 'System', command: 'uname -a' },
		{ name: 'List files', command: 'ls -la' }
	];

	const pane = usePane();
	/** Component instances by session id (kept out of reactive state). */
	const views: Record<string, TerminalView | undefined> = {};
	const firstId = uid();
	let sessions = $state<Session[]>([{ id: firstId, target: { kind: 'host' }, running: false }]);
	let activeId = $state(firstId);
	let editing = $state<string | null>(null);
	let commandsOpen = $state(false);
	let commands = $state<SavedCommand[] | null>(null);
	let commandFilter = $state('');
	let pathDialog = $state(false);
	let pathValue = $state('');
	let commandForm = $state(false);
	let draft = $state<SavedCommand>({ id: '', name: '', command: '' });

	const active = $derived(sessions.find((s) => s.id === activeId) ?? sessions[0]);
	const label = (s: Session) => (s.target.kind === 'container' ? s.target.container : 'Server shell');

	function addSession(target: TerminalTarget): void {
		const session: Session = { id: uid(), target, running: false };
		sessions.push(session);
		activeId = session.id;
		editing = null;
	}

	async function closeSession(session: Session): Promise<void> {
		if (session.running) {
			const ok = await confirm({
				title: `Close “${label(session)}”?`,
				message: 'The shell and anything running in it will be terminated.',
				confirmLabel: 'Close terminal',
				destructive: true
			});
			if (!ok) return;
		}
		sessions = sessions.filter((s) => s.id !== session.id);
		if (sessions.length === 0) addSession({ kind: 'host' });
		else if (activeId === session.id) activeId = sessions[0].id;
	}

	// Requests from other tabs: container shells and "edit this file".
	$effect(() => {
		if (!visible) return;
		const request = workspace.takeRequest('terminal');
		if (!request) return;
		if (request.kind === 'container')
			addSession({ kind: 'container', container: request.container, shell: request.shell });
		else editing = request.path;
	});

	// A live shell makes closing the pane ask first.
	$effect(() => {
		const live = sessions.filter((s) => s.running).length;
		pane?.setLive('terminal', live > 0 ? `${live} running terminal session${live === 1 ? '' : 's'}` : null);
		return () => pane?.setLive('terminal', null);
	});

	async function openPathDialog(): Promise<void> {
		const session = views[active.id]?.sessionId();
		let cwd = '';
		if (session && active.target.kind === 'host')
			cwd = (await api.terminalCwd(session).catch(() => null)) ?? '';
		pathValue = cwd ? `${cwd.replace(/\/$/, '')}/` : '';
		pathDialog = true;
	}

	async function loadCommands(): Promise<void> {
		try {
			commands = await loadDoc(
				'savedCommands',
				DEFAULT_COMMANDS.map((c) => ({ ...c, id: uid() }))
			);
		} catch (error) {
			toast.error(error);
			commands = [];
		}
	}

	async function persistCommands(next: SavedCommand[]): Promise<void> {
		commands = next;
		try {
			await saveDoc('savedCommands', next);
		} catch (error) {
			toast.error(error, 'Could not save commands');
		}
	}

	function toggleCommands(): void {
		commandsOpen = !commandsOpen;
		if (commandsOpen && commands === null) void loadCommands();
	}

	async function saveCommand(): Promise<void> {
		if (!draft.name.trim() || !draft.command.trim()) return;
		const all = commands ?? [];
		const entry = { ...draft, id: draft.id || uid() };
		await persistCommands(draft.id ? all.map((c) => (c.id === draft.id ? entry : c)) : [...all, entry]);
		commandForm = false;
	}

	async function external(): Promise<void> {
		try {
			await api.terminalOpenExternal();
		} catch (error) {
			toast.error(error);
		}
	}

	export function refresh(): void {
		views[active.id]?.focus();
	}

	export function onReselect(): void {
		editing = null;
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<header class="flex h-9 shrink-0 items-center gap-1 border-b bg-card px-1.5">
		<div class="flex min-w-0 items-center gap-0.5 overflow-x-auto" role="tablist">
			{#each sessions as session (session.id)}
				<div
					class={cn(
						'group flex h-7 shrink-0 items-center gap-1.5 rounded-md pr-1 pl-2 text-xs',
						session.id === activeId && !editing
							? 'bg-muted font-medium'
							: 'text-muted-foreground hover:bg-muted/60'
					)}
				>
					<button
						type="button"
						role="tab"
						aria-selected={session.id === activeId}
						class="flex items-center gap-1.5"
						onclick={() => {
							activeId = session.id;
							editing = null;
						}}
					>
						{#if session.target.kind === 'container'}<Box class="size-3.5 text-info" />{/if}
						<span
							class={cn('size-1.5 rounded-full', session.running ? 'bg-success' : 'bg-muted-foreground/50')}
						></span>
						{label(session)}
					</button>
					<button
						type="button"
						class="rounded p-0.5 opacity-60 hover:text-foreground hover:opacity-100"
						aria-label="Close terminal"
						onclick={() => closeSession(session)}
					>
						<X class="size-3" />
					</button>
				</div>
			{/each}
		</div>
		<IconButton label="New server shell" size="icon-xs" onclick={() => addSession({ kind: 'host' })}
			><Plus /></IconButton
		>
		{#if active.target.kind === 'container'}
			<Button variant="ghost" size="xs" onclick={() => addSession({ kind: 'host' })}
				><ArrowLeft /> Back to server shell</Button
			>
		{/if}
		<span class="flex-1"></span>
		<IconButton label="Restart session" onclick={() => views[active.id]?.restart()}><RotateCw /></IconButton>
		<IconButton label="Edit file" onclick={openPathDialog}><FilePen /></IconButton>
		<IconButton label="Saved commands" variant={commandsOpen ? 'secondary' : 'ghost'} onclick={toggleCommands}
			><Bookmark /></IconButton
		>
		<IconButton label="Open in external terminal" onclick={external}><ExternalLink /></IconButton>
		<IconButton label="Terminal settings" onclick={() => workspace.openSettings('terminal')}
			><Settings /></IconButton
		>
	</header>

	<div class="flex min-h-0 flex-1">
		<div class="relative min-w-0 flex-1">
			{#each sessions as session (session.id)}
				<TerminalView
					bind:this={views[session.id]}
					target={session.target}
					active={visible && session.id === activeId && !editing}
					onstatus={(running) => (session.running = running)}
					onedit={(path) => (editing = path)}
				/>
			{/each}
			{#if editing}
				<div class="absolute inset-0 z-10 bg-background">
					{#key editing}
						<FileEditor path={editing} onclose={() => (editing = null)} />
					{/key}
				</div>
			{/if}
		</div>

		{#if commandsOpen}
			<aside class="flex w-72 shrink-0 flex-col border-l bg-card">
				<div class="flex items-center gap-1 border-b p-2">
					<SearchInput bind:value={commandFilter} placeholder="Filter commands…" class="min-w-0 flex-1" />
					<IconButton
						label="Add command"
						onclick={() => {
							draft = { id: '', name: '', command: '' };
							commandForm = true;
						}}
					>
						<Plus />
					</IconButton>
				</div>
				<ul class="min-h-0 flex-1 overflow-y-auto p-1">
					{#each (commands ?? []).filter( (c) => matches(commandFilter, c.name, c.command) ) as command (command.id)}
						<li class="group flex items-center gap-1 rounded-md px-2 py-1.5 hover:bg-muted/60">
							<button
								type="button"
								class="min-w-0 flex-1 text-left"
								title="Run in terminal"
								onclick={() => views[active.id]?.run(command.command)}
							>
								<p class="truncate text-sm">{command.name}</p>
								<p class="truncate font-mono text-[11px] text-muted-foreground">{command.command}</p>
							</button>
							<IconButton label="Run" size="icon-xs" onclick={() => views[active.id]?.run(command.command)}
								><Play /></IconButton
							>
							<IconButton
								label="Edit"
								size="icon-xs"
								onclick={() => {
									draft = { ...command };
									commandForm = true;
								}}
							>
								<Pencil />
							</IconButton>
							<IconButton
								label="Delete"
								size="icon-xs"
								onclick={() => persistCommands((commands ?? []).filter((c) => c.id !== command.id))}
							>
								<Trash2 />
							</IconButton>
						</li>
					{:else}
						<li class="p-3 text-xs text-muted-foreground">
							{commands === null ? 'Loading…' : 'No saved commands.'}
						</li>
					{/each}
				</ul>
			</aside>
		{/if}
	</div>
</div>

<Modal
	bind:open={pathDialog}
	title="Edit file"
	description="Open a file on the server in the editor."
	size="md"
>
	<PathInput
		bind:value={pathValue}
		autofocus
		onsubmit={(path) => {
			if (path.startsWith('/')) {
				pathDialog = false;
				editing = path;
			}
		}}
	/>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (pathDialog = false)}>Cancel</Button>
		<Button
			disabled={!pathValue.trim().startsWith('/')}
			onclick={() => {
				pathDialog = false;
				editing = pathValue.trim();
			}}
		>
			Open
		</Button>
	{/snippet}
</Modal>

<Modal bind:open={commandForm} title={draft.id ? 'Edit command' : 'Add command'} size="md">
	<form
		id="saved-command-form"
		class="flex flex-col gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void saveCommand();
		}}
	>
		<Field label="Name" required><Input bind:value={draft.name} autofocus /></Field>
		<Field label="Command" required
			><Input bind:value={draft.command} class="font-mono text-xs" spellcheck="false" /></Field
		>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (commandForm = false)}>Cancel</Button>
		<Button type="submit" form="saved-command-form" disabled={!draft.name.trim() || !draft.command.trim()}
			>Save</Button
		>
	{/snippet}
</Modal>
