<script lang="ts">
	import { parseDocument } from 'yaml';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Download from '@lucide/svelte/icons/download';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Play from '@lucide/svelte/icons/play';
	import Plus from '@lucide/svelte/icons/plus';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import ScrollText from '@lucide/svelte/icons/scroll-text';
	import Square from '@lucide/svelte/icons/square';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import FileEditor from '$lib/editor/FileEditor.svelte';
	import { api, type ComposeAction, type ComposeProject } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';
	import { usePane } from '$lib/workspace/workspace.svelte';

	let { visible, onchanged }: { visible: boolean; onchanged: () => void } = $props();

	const STARTER = `services:
  app:
    image: nginx:stable
    restart: unless-stopped
    ports:
      - "8080:80"
`;

	const projects = resource((io) => io.composeList());
	const busy = new Busy();
	const pane = usePane();
	let search = $state('');
	let selected = $state(new Set<string>());
	let editing = $state<ComposeProject | null>(null);
	let logs = $state<{ project: ComposeProject; job: Job | null } | null>(null);
	let output = $state<{ title: string; job: Job } | null>(null);
	let createOpen = $state(false);
	let form = $state({ directory: '', folder: '', content: STARTER });
	let networkFor = $state<ComposeProject | null>(null);
	let networks = $state<string[]>([]);
	let network = $state('');

	autoLoad(projects, () => visible && !editing && !logs, 15_000);

	const key = (p: ComposeProject) => p.configFile || p.name;
	const columns: Column<ComposeProject>[] = [
		{ key: 'name', label: 'Project', value: (p) => p.name, class: 'w-64 max-w-64' },
		{ key: 'status', label: 'Status', value: (p) => p.status || 'down', class: 'w-44' },
		{
			key: 'configFile',
			label: 'Compose file',
			value: (p) => p.configFile,
			mono: true,
			class: 'max-w-0 w-full'
		}
	];
	const LABEL: Record<ComposeAction, string> = { up: 'Up', down: 'Down', restart: 'Restart', pull: 'Pull' };

	/** Run an action for several projects; each is its own job in Running Jobs. */
	async function act(list: ComposeProject[], action: ComposeAction): Promise<void> {
		if (action === 'down') {
			const ok = await confirm({
				title: list.length === 1 ? `Bring “${list[0].name}” down?` : `Bring ${list.length} stacks down?`,
				message: 'Containers and networks of the stack are removed. Volumes are kept.',
				confirmLabel: 'Compose down',
				destructive: true
			});
			if (!ok) return;
		}
		selected = new Set();
		await Promise.all(
			list.map((project) =>
				busy.run(
					key(project),
					async () => {
						const job = jobs.get(await api.composeAction(project.name, project.configFile, action));
						if (list.length === 1) output = { title: `${LABEL[action]} · ${project.name}`, job };
						const result = await job.wait();
						if (result.status === 'failed')
							throw result.error ?? new Error(`Compose ${action} failed for ${project.name}`);
					},
					(e) => toast.error(e, `${LABEL[action]} failed for ${project.name}`)
				)
			)
		);
		await projects.load(true);
		onchanged();
	}

	// Follow logs while the logs view is open.
	$effect(() => {
		const view = logs;
		if (!view || !visible) return;
		let current: Job | null = null;
		let cancelled = false;
		api.composeLogs(view.project.name, view.project.configFile).then(
			(id) => {
				current = jobs.get(id);
				if (cancelled) void current.stop();
				else view.job = current;
			},
			(e) => toast.error(e)
		);
		return () => {
			cancelled = true;
			void current?.stop();
		};
	});
	$effect(() => {
		if (logs) return pane?.pushBack('Back to stacks', () => (logs = null));
	});

	async function validateAfterSave(project: ComposeProject): Promise<void> {
		const problem = await api.composeValidate(project.configFile);
		if (problem) toast.warning('Saved, but Docker Compose reports a problem', problem);
		else toast.success('Saved. Run “Up” to apply the changes.');
	}

	async function forget(project: ComposeProject): Promise<void> {
		const ok = await confirm({
			title: `Forget “${project.name}”?`,
			message: 'The stack is removed from this list only. Its files and containers are not touched.',
			confirmLabel: 'Forget'
		});
		if (!ok) return;
		try {
			await api.composeForget(project.configFile);
			await projects.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	async function create(): Promise<void> {
		try {
			const file = await api.composeCreate(form.directory.trim(), form.folder.trim(), form.content);
			toast.success('Project created');
			createOpen = false;
			await projects.refresh();
			editing = projects.data?.find((p) => p.configFile === file) ?? null;
		} catch (error) {
			toast.error(error, 'Could not create the project');
		}
	}

	async function openNetwork(project: ComposeProject): Promise<void> {
		try {
			networks = (await api.dockerNetworks())
				.map((n) => n.name)
				.filter((n) => !['bridge', 'host', 'none'].includes(n));
			network = networks[0] ?? '';
			networkFor = project;
		} catch (error) {
			toast.error(error);
		}
	}

	/** Point the stack's default network at an existing external network, keeping comments intact. */
	async function attachNetwork(): Promise<void> {
		const project = networkFor;
		if (!project || !network) return;
		try {
			const file = await api.filesRead(project.configFile);
			const doc = parseDocument(file.content);
			if (doc.errors.length) throw new Error(doc.errors[0].message);
			doc.setIn(['networks', 'default'], doc.createNode({ name: network, external: true }));
			await api.filesWrite(project.configFile, doc.toString());
			networkFor = null;
			toast.success(`Default network set to “${network}”. Run “Up” to apply.`);
		} catch (error) {
			toast.error(error, 'Could not change the network');
		}
	}

	export const refresh = () => projects.refresh();
	/** Quiet reload after something changed outside this view. */
	export const sync = () => projects.load(true);
	export function reset(): void {
		editing = null;
		logs = null;
	}
</script>

{#if editing}
	{@const project = editing}
	<FileEditor
		path={project.configFile}
		language="yaml"
		onclose={() => {
			editing = null;
			void projects.load(true);
		}}
		aftersave={() => validateAfterSave(project)}
	/>
{:else if logs}
	<div class="flex h-full min-h-0 flex-col gap-2 p-4">
		<div class="flex items-center gap-2">
			<IconButton label="Back to stacks" variant="outline" onclick={() => (logs = null)}
				><ArrowLeft /></IconButton
			>
			<h2 class="truncate text-[15px] font-semibold">Logs · {logs.project.name}</h2>
		</div>
		<LogViewer
			source={logs.job}
			severity
			live
			placeholder="Waiting for log output…"
			downloadName="{logs.project.name}.log"
			class="flex-1"
		/>
	</div>
{:else}
	<Page scroll={false}>
		{#snippet toolbar()}
			<SearchInput bind:value={search} placeholder="Search stacks…" class="w-64" />
			<span class="flex-1"></span>
			<Button
				size="sm"
				onclick={() => {
					form = { directory: '', folder: '', content: STARTER };
					createOpen = true;
				}}><Plus /> New project</Button
			>
			<RefreshControl onrefresh={projects.refresh} loading={projects.loading} />
		{/snippet}
		<DataTable
			rows={projects.data ?? []}
			{columns}
			rowKey={key}
			{search}
			selectable
			bind:selected
			busy={busy.keys}
			loading={projects.loading}
			error={projects.error}
			onretry={projects.refresh}
			empty="No compose projects"
			emptyHint="Projects appear here once they have been started on this server, or create a new one."
			class="flex-1"
		>
			{#snippet cell(project, column)}
				{#if column.key === 'status'}
					<span class="flex items-center gap-2">
						<span class={cn('size-2 rounded-full', project.running ? 'bg-success' : 'bg-muted-foreground/40')}
						></span>
						{project.status || 'down'}
					</span>
				{:else if column.key === 'name'}
					<span class="font-medium">{project.name}</span>
				{:else}
					{column.value?.(project)}
				{/if}
			{/snippet}
			{#snippet actions(project)}
				<IconButton label="Up (create and start)" onclick={() => act([project], 'up')}><Play /></IconButton>
				<IconButton label="Restart" onclick={() => act([project], 'restart')}><RotateCw /></IconButton>
				<IconButton label="Down" onclick={() => act([project], 'down')}><Square /></IconButton>
				<IconButton label="Pull images" onclick={() => act([project], 'pull')}><Download /></IconButton>
				<IconButton label="Logs" onclick={() => (logs = { project, job: null })}><ScrollText /></IconButton>
				<IconButton label="Edit compose file" onclick={() => (editing = project)}><Pencil /></IconButton>
			{/snippet}
			{#snippet bulk(rows)}
				<Button variant="outline" size="xs" onclick={() => act(rows, 'up')}>Up</Button>
				<Button variant="outline" size="xs" onclick={() => act(rows, 'restart')}>Restart</Button>
				<Button variant="outline" size="xs" onclick={() => act(rows, 'pull')}>Pull</Button>
				<Button variant="destructive" size="xs" onclick={() => act(rows, 'down')}>Down</Button>
			{/snippet}
			{#snippet menu(project)}
				<ContextMenu.Item onclick={() => act([project], 'up')}>Up</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act([project], 'restart')}>Restart</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act([project], 'pull')}>Pull images</ContextMenu.Item>
				<ContextMenu.Item onclick={() => act([project], 'down')}>Down</ContextMenu.Item>
				<ContextMenu.Separator />
				<ContextMenu.Item onclick={() => (logs = { project, job: null })}>Logs</ContextMenu.Item>
				<ContextMenu.Item onclick={() => (editing = project)}>Edit compose file</ContextMenu.Item>
				<ContextMenu.Item onclick={() => openNetwork(project)}>Attach external network…</ContextMenu.Item>
				<ContextMenu.Separator />
				<ContextMenu.Item onclick={() => forget(project)}>Forget stack</ContextMenu.Item>
			{/snippet}
		</DataTable>
	</Page>
{/if}

{#if output}
	<Modal open title={output.title} size="xl" class="h-[70vh]" flush onclose={() => (output = null)}>
		<LogViewer source={output.job} downloadName="compose.log" class="flex-1" />
	</Modal>
{/if}

<Modal bind:open={createOpen} title="New compose project" size="lg">
	<form
		id="compose-form"
		class="flex flex-col gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void create();
		}}
	>
		<div class="grid grid-cols-2 gap-3">
			<Field label="Parent directory" required
				><PathInput bind:value={form.directory} dirsOnly placeholder="/srv" /></Field
			>
			<Field label="Folder name" required hint="Also becomes the project name."
				><Input bind:value={form.folder} placeholder="my-app" spellcheck="false" /></Field
			>
		</div>
		<Field label="docker-compose.yml"
			><Textarea bind:value={form.content} rows={10} class="font-mono text-xs" spellcheck="false" /></Field
		>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
		<Button
			type="submit"
			form="compose-form"
			disabled={!form.directory.trim().startsWith('/') || !/^[A-Za-z0-9_.-]+$/.test(form.folder)}
			>Create project</Button
		>
	{/snippet}
</Modal>

{#if networkFor}
	<Modal
		open
		title="Attach external network · {networkFor.name}"
		description="Sets the stack's default network to an existing Docker network in the compose file."
		size="md"
		onclose={() => (networkFor = null)}
	>
		{#if networks.length}
			<SelectField bind:value={network} options={networks} />
		{:else}
			<p class="text-sm text-muted-foreground">
				There are no user-defined networks. Create one in the Networks tab first.
			</p>
		{/if}
		{#snippet footer()}
			<Button variant="outline" onclick={() => (networkFor = null)}>Cancel</Button>
			<Button disabled={!network} onclick={attachNetwork}>Attach</Button>
		{/snippet}
	</Modal>
{/if}
