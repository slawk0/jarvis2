<!-- Floating panel for the transfer engine, plus the conflict dialog. -->
<script lang="ts">
	import ArrowDownToLine from '@lucide/svelte/icons/arrow-down-to-line';
	import ArrowUpFromLine from '@lucide/svelte/icons/arrow-up-from-line';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronUp from '@lucide/svelte/icons/chevron-up';
	import Copy from '@lucide/svelte/icons/copy';
	import FolderInput from '@lucide/svelte/icons/folder-input';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import X from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { formatBytes, formatSpeed } from '$lib/format';
	import { toIpcError, type TransferJob } from '$lib/ipc';
	import { transfers } from '$lib/services/transfers.svelte';
	import { cn } from '$lib/utils';
	import IconButton from './IconButton.svelte';
	import Modal from './Modal.svelte';
	import SelectField from './SelectField.svelte';

	let collapsed = $state(false);
	let order = $state<'queue' | 'name' | 'status'>('queue');

	const ICONS = {
		upload: ArrowUpFromLine,
		download: ArrowDownToLine,
		move: FolderInput,
		copy: Copy,
		delete: Trash2
	};
	const RANK = { running: 0, queued: 1, failed: 2, cancelled: 3, skipped: 4, done: 5 };
	const totals = $derived(transfers.totals);
	const sorted = $derived.by(() => {
		const list = [...transfers.jobs];
		if (order === 'name') list.sort((a, b) => a.name.localeCompare(b.name));
		if (order === 'status') list.sort((a, b) => RANK[a.status] - RANK[b.status]);
		return list;
	});
	const hasFailed = $derived(transfers.jobs.some((j) => j.status === 'failed' || j.status === 'cancelled'));

	const percent = (job: TransferJob) =>
		job.status === 'done' ? 100 : job.size > 0 ? Math.min(100, (job.transferred / job.size) * 100) : 0;
	const sized = (job: TransferJob) => job.kind === 'upload' || job.kind === 'download';
</script>

{#if transfers.panelOpen && transfers.jobs.length > 0}
	<section
		class={cn(
			'fixed bottom-4 left-64 z-40 flex w-[28rem] max-w-[calc(100vw-2rem)] flex-col overflow-hidden rounded-xl border bg-popover shadow-2xl',
			collapsed ? '' : 'max-h-[24rem]'
		)}
		aria-label="Transfers"
	>
		<header class="flex shrink-0 items-center gap-2 border-b px-3 py-2">
			<h2 class="text-sm font-semibold">Transfers</h2>
			<span class="text-xs text-muted-foreground tabular">
				{totals.done}/{totals.total} done{#if totals.errors}
					· <span class="text-destructive">{totals.errors} error{totals.errors === 1 ? '' : 's'}</span>{/if}
				{#if totals.speed > 0}
					· {formatSpeed(totals.speed)}{/if}
			</span>
			<span class="flex-1"></span>
			<IconButton label={collapsed ? 'Expand' : 'Collapse'} onclick={() => (collapsed = !collapsed)}>
				{#if collapsed}<ChevronUp />{:else}<ChevronDown />{/if}
			</IconButton>
			<IconButton label="Close" onclick={() => (transfers.panelOpen = false)}><X /></IconButton>
		</header>
		{#if !collapsed}
			<ul class="min-h-0 flex-1 overflow-y-auto">
				{#each sorted as job (job.id)}
					{@const Icon = ICONS[job.kind]}
					<li class="border-b px-3 py-2 last:border-b-0">
						<div class="flex items-center gap-2">
							<Icon class="size-3.5 shrink-0 text-muted-foreground" />
							<span
								class="min-w-0 flex-1 truncate text-sm"
								title={job.kind === 'upload' ? job.remote : job.kind === 'download' ? job.local : job.remote}
							>
								{job.name}
							</span>
							{#if job.elevated}<ShieldAlert
									class="size-3.5 shrink-0 text-warning"
									aria-label="Transferred as root"
								/>{/if}
							<span
								class={cn(
									'shrink-0 text-xs tabular',
									job.status === 'failed'
										? 'text-destructive'
										: job.status === 'done'
											? 'text-success'
											: 'text-muted-foreground'
								)}
							>
								{#if job.status === 'running' && sized(job)}
									{formatBytes(job.transferred)} / {formatBytes(job.size)} · {formatSpeed(job.speed)}
								{:else if job.status === 'running'}
									working…
								{:else}
									{job.status}{#if job.status === 'done' && sized(job)}
										· {formatBytes(job.size)}{/if}
								{/if}
							</span>
							{#if job.status === 'queued' || job.status === 'running'}
								<IconButton label="Cancel" size="icon-xs" onclick={() => transfers.cancel(job.id)}
									><X /></IconButton
								>
							{/if}
						</div>
						{#if job.status === 'running' && sized(job)}
							<div class="mt-1.5 h-1 overflow-hidden rounded-full bg-muted">
								<div class="h-full bg-primary transition-[width]" style="width: {percent(job)}%"></div>
							</div>
						{/if}
						{#if job.error}
							<p class="selectable mt-1 text-xs break-words text-destructive">
								{toIpcError(job.error).message}
							</p>
						{/if}
					</li>
				{/each}
			</ul>
			<footer class="flex shrink-0 items-center gap-1.5 border-t px-2 py-1.5">
				<SelectField
					size="sm"
					class="w-28"
					bind:value={order}
					options={[
						{ value: 'queue', label: 'Queue order' },
						{ value: 'name', label: 'By name' },
						{ value: 'status', label: 'By status' }
					]}
				/>
				<span class="flex-1"></span>
				{#if transfers.active.length > 0}
					<Button variant="ghost" size="xs" onclick={() => transfers.cancel()}>Cancel all</Button>
				{/if}
				{#if hasFailed}
					<Button variant="ghost" size="xs" onclick={() => transfers.retryFailed()}>Retry failed</Button>
				{/if}
				<Button variant="ghost" size="xs" onclick={() => transfers.clearCompleted()}>Clear completed</Button>
			</footer>
		{/if}
	</section>
{/if}

{#if transfers.conflict}
	{@const conflict = transfers.conflict}
	<Modal open title="Some items already exist" size="md" onclose={() => conflict.resolve(null)}>
		<p class="mb-2 text-sm text-muted-foreground">
			{conflict.paths.length} target{conflict.paths.length === 1 ? '' : 's'} already exist{conflict.paths
				.length === 1
				? 's'
				: ''}. Your choice applies to all of them.
		</p>
		<pre
			class="selectable max-h-40 overflow-auto rounded-md border bg-sunken p-2 text-xs">{conflict.paths.join(
				'\n'
			)}</pre>
		{#snippet footer()}
			<Button variant="outline" onclick={() => conflict.resolve(null)}>Cancel</Button>
			<Button variant="outline" onclick={() => conflict.resolve('skip')}>Skip existing</Button>
			<Button variant="outline" onclick={() => conflict.resolve('rename')}>Keep both</Button>
			<Button variant="destructive" onclick={() => conflict.resolve('overwrite')}>Overwrite</Button>
		{/snippet}
	</Modal>
{/if}
