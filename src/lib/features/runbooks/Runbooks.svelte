<script lang="ts">
	import Pencil from '@lucide/svelte/icons/pencil';
	import Play from '@lucide/svelte/icons/play';
	import Plus from '@lucide/svelte/icons/plus';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Square from '@lucide/svelte/icons/square';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import X from '@lucide/svelte/icons/x';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { api } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { loadDoc, saveDoc, type Runbook } from '$lib/services/profile-data';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { resource } from '$lib/state/resource.svelte';
	import { uid } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';

	let { visible }: TabProps = $props();

	const PRESETS: Omit<Runbook, 'id'>[] = [
		{ name: 'Docker prune', command: 'docker system prune -f', sudo: false },
		{ name: 'Compose status', command: 'docker compose ls --all', sudo: false },
		{ name: 'Nginx test', command: 'nginx -t', sudo: true },
		{ name: 'Nginx reload', command: 'nginx -t && systemctl reload nginx', sudo: true },
		{ name: 'Nginx restart', command: 'systemctl restart nginx', sudo: true },
		{ name: 'Disk usage', command: 'df -h && du -sh /var/log/* 2>/dev/null | sort -h | tail', sudo: false },
		{ name: 'Failed units', command: 'systemctl --failed --no-pager', sudo: false }
	];

	const runbooks = resource(() => loadDoc('runbooks', []));
	/** Latest job per runbook. */
	let runs = $state<Record<string, Job>>({});
	let shownId = $state<string | null>(null);
	let formOpen = $state(false);
	let draft = $state<Runbook>({ id: '', name: '', command: '', sudo: false });
	let submitted = $state(false);

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void runbooks.refresh();
		}
	});

	const shown = $derived(shownId ? runs[shownId] : null);
	const shownBook = $derived(runbooks.data?.find((r) => r.id === shownId));

	async function persist(next: Runbook[]): Promise<void> {
		try {
			await saveDoc('runbooks', next);
			runbooks.set(next);
		} catch (error) {
			toast.error(error, 'Could not save runbooks');
		}
	}

	function openForm(book?: Omit<Runbook, 'id'> & { id?: string }): void {
		draft = {
			id: book?.id ?? '',
			name: book?.name ?? '',
			command: book?.command ?? '',
			sudo: book?.sudo ?? false
		};
		submitted = false;
		formOpen = true;
	}

	async function save(): Promise<void> {
		submitted = true;
		if (!draft.name.trim() || !draft.command.trim()) return;
		const all = runbooks.data ?? [];
		const entry = { ...draft, name: draft.name.trim(), id: draft.id || uid() };
		await persist(draft.id ? all.map((r) => (r.id === draft.id ? entry : r)) : [...all, entry]);
		formOpen = false;
	}

	async function remove(book: Runbook): Promise<void> {
		const ok = await confirm({
			title: `Delete runbook “${book.name}”?`,
			confirmLabel: 'Delete',
			destructive: true
		});
		if (ok) await persist((runbooks.data ?? []).filter((r) => r.id !== book.id));
	}

	async function run(book: Runbook): Promise<void> {
		if (runs[book.id]?.running) return;
		try {
			const jobId = await sudo.describe(`Runbook “${book.name}”`, () =>
				api.runbookRun(book.name, book.command, book.sudo)
			);
			runs[book.id] = jobs.get(jobId);
			shownId = book.id;
		} catch (error) {
			toast.error(error);
		}
	}

	export const refresh = () => runbooks.refresh();
	export function onReselect(): void {
		shownId = null;
	}
</script>

<Page scroll={false}>
	{#snippet toolbar()}
		<p class="mr-auto text-xs text-muted-foreground">
			Saved commands for this server. Output is streamed live.
		</p>
		<Button size="sm" onclick={() => openForm()}><Plus /> New runbook</Button>
	{/snippet}

	<div class="flex min-h-0 flex-1 gap-3">
		<div class="min-h-0 min-w-0 flex-1 overflow-y-auto">
			{#if runbooks.error}
				<StateView kind="error" error={runbooks.error} onretry={runbooks.refresh} />
			{:else if !runbooks.loaded}
				<StateView kind="loading" />
			{:else if runbooks.data?.length === 0}
				<StateView
					kind="empty"
					title="No runbooks yet"
					message="Create one from scratch or start from a preset below."
				>
					<Button size="sm" onclick={() => openForm()}><Plus /> New runbook</Button>
				</StateView>
			{:else}
				<div class="grid grid-cols-[repeat(auto-fill,minmax(18rem,1fr))] gap-3">
					{#each runbooks.data ?? [] as book (book.id)}
						{@const job = runs[book.id]}
						<article class="flex flex-col gap-2 rounded-xl border bg-card p-3">
							<div class="flex items-center gap-2">
								<h3 class="min-w-0 flex-1 truncate text-sm font-semibold">{book.name}</h3>
								{#if book.sudo}
									<span class="flex items-center gap-1 text-[11px] text-warning"
										><ShieldCheck class="size-3.5" /> sudo</span
									>
								{/if}
							</div>
							<pre
								class="selectable max-h-24 overflow-auto rounded-md border bg-sunken px-2 py-1.5 text-[11px] whitespace-pre-wrap text-muted-foreground">{book.command}</pre>
							<div class="mt-auto flex items-center gap-1">
								{#if job?.running}
									<Button variant="destructive" size="sm" onclick={() => job.stop()}><Square /> Stop</Button>
								{:else}
									<Button size="sm" onclick={() => run(book)}><Play /> Run</Button>
								{/if}
								{#if job}
									<button
										type="button"
										class="ml-1 text-xs text-muted-foreground hover:text-foreground"
										onclick={() => (shownId = book.id)}
									>
										{#if job.running}Running…{:else if job.status === 'done'}<span class="text-success"
												>Exit 0</span
											>{:else if job.status === 'cancelled'}Stopped{:else}<span class="text-destructive"
												>Exit {job.exitCode ?? '?'}</span
											>{/if}
									</button>
								{/if}
								<span class="flex-1"></span>
								<IconButton label="Edit" onclick={() => openForm(book)}><Pencil /></IconButton>
								<IconButton label="Delete" onclick={() => remove(book)}><Trash2 /></IconButton>
							</div>
						</article>
					{/each}
				</div>
			{/if}

			{#if runbooks.loaded}
				<h3 class="mt-5 mb-2 text-xs font-medium text-muted-foreground">Quick presets</h3>
				<div class="flex flex-wrap gap-1.5">
					{#each PRESETS as preset (preset.name)}
						<Button variant="outline" size="xs" onclick={() => openForm(preset)}>{preset.name}</Button>
					{/each}
				</div>
			{/if}
		</div>

		{#if shown}
			<aside class="flex min-h-0 w-[45%] max-w-2xl min-w-80 flex-col gap-2">
				<div class="flex items-center gap-2">
					<h3 class="min-w-0 flex-1 truncate text-sm font-semibold">Output · {shownBook?.name}</h3>
					<span class="text-xs text-muted-foreground">
						{#if shown.running}running…{:else if shown.exitCode !== null}exit code {shown.exitCode}{:else}{shown.status}{/if}
					</span>
					<IconButton label="Close output" onclick={() => (shownId = null)}><X /></IconButton>
				</div>
				{#if shown.error}<p class="selectable text-xs text-destructive">{shown.error.message}</p>{/if}
				<LogViewer source={shown} downloadName="{shownBook?.name ?? 'runbook'}.log" class="flex-1" />
			</aside>
		{/if}
	</div>
</Page>

<Modal bind:open={formOpen} title={draft.id ? 'Edit runbook' : 'New runbook'} size="lg">
	<form
		id="runbook-form"
		class="flex flex-col gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void save();
		}}
	>
		<Field label="Name" required error={submitted && !draft.name.trim() ? 'Give the runbook a name.' : null}>
			<Input bind:value={draft.name} placeholder="Restart application" autofocus />
		</Field>
		<Field
			label="Command"
			required
			hint="Runs with sh on the server. Multiple lines are allowed."
			error={submitted && !draft.command.trim() ? 'Enter the command to run.' : null}
		>
			<Textarea bind:value={draft.command} rows={7} class="font-mono text-xs" spellcheck="false" />
		</Field>
		<label class="flex items-center gap-2 text-sm">
			<Checkbox bind:checked={draft.sudo} /> Run as root (sudo)
		</label>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (formOpen = false)}>Cancel</Button>
		<Button type="submit" form="runbook-form">Save</Button>
	{/snippet}
</Modal>
