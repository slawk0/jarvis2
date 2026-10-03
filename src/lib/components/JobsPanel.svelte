<!-- The global "Running Jobs" floating panel. -->
<script lang="ts">
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronUp from '@lucide/svelte/icons/chevron-up';
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import CircleSlash from '@lucide/svelte/icons/circle-slash';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Square from '@lucide/svelte/icons/square';
	import X from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { formatClock } from '$lib/format';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { cn } from '$lib/utils';
	import IconButton from './IconButton.svelte';
	import LogViewer from './LogViewer.svelte';

	let collapsed = $state(false);
	let now = $state(Date.now());

	const list = $derived(jobs.visible);
	const selected = $derived(list.find((j) => j.id === jobs.selectedId) ?? null);
	const counts = $derived({
		running: list.filter((j) => j.status === 'running').length,
		done: list.filter((j) => j.status === 'done').length,
		failed: list.filter((j) => j.status === 'failed' || j.status === 'cancelled').length
	});

	// Tick the clock only while something is running and the panel is open.
	$effect(() => {
		if (!jobs.panelOpen || counts.running === 0) return;
		const timer = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(timer);
	});

	const duration = (job: Job) => formatClock((job.endedAt ?? now) - job.startedAt);
</script>

{#snippet statusIcon(job: Job)}
	{#if job.status === 'running'}
		<LoaderCircle class="text-info size-4 shrink-0 animate-spin" />
	{:else if job.status === 'done'}
		<CircleCheck class="text-success size-4 shrink-0" />
	{:else if job.status === 'cancelled'}
		<CircleSlash class="text-muted-foreground size-4 shrink-0" />
	{:else}
		<CircleX class="text-destructive size-4 shrink-0" />
	{/if}
{/snippet}

{#if jobs.panelOpen}
	<section
		class={cn(
			'bg-popover fixed right-4 bottom-4 z-40 flex w-[30rem] max-w-[calc(100vw-2rem)] flex-col overflow-hidden rounded-xl border shadow-2xl',
			collapsed ? 'h-auto' : 'h-[26rem] max-h-[calc(100vh-6rem)]'
		)}
		aria-label="Running jobs"
	>
		<header class="flex shrink-0 items-center gap-2 border-b px-3 py-2">
			{#if selected}
				<IconButton label="Back to job list" onclick={() => (jobs.selectedId = null)}><ArrowLeft /></IconButton>
				{@render statusIcon(selected)}
				<h2 class="min-w-0 flex-1 truncate text-sm font-semibold">{selected.title}</h2>
			{:else}
				<h2 class="text-sm font-semibold">Running jobs</h2>
				<div class="text-muted-foreground tabular flex items-center gap-2 text-xs">
					<span class={cn(counts.running > 0 && 'text-info')}>{counts.running} running</span>
					<span>{counts.done} completed</span>
					<span class={cn(counts.failed > 0 && 'text-destructive')}>{counts.failed} failed</span>
				</div>
				<span class="flex-1"></span>
			{/if}
			<IconButton label={collapsed ? 'Expand' : 'Collapse'} onclick={() => (collapsed = !collapsed)}>
				{#if collapsed}<ChevronUp />{:else}<ChevronDown />{/if}
			</IconButton>
			<IconButton label="Close" onclick={() => (jobs.panelOpen = false)}><X /></IconButton>
		</header>

		{#if !collapsed}
			{#if selected}
				<div class="flex min-h-0 flex-1 flex-col gap-2 p-2">
					{#if selected.detail}
						<pre class="bg-sunken selectable text-muted-foreground max-h-16 shrink-0 overflow-auto rounded-md border px-2 py-1.5 text-[11px] whitespace-pre-wrap">{selected.detail}</pre>
					{/if}
					<LogViewer source={selected} downloadName="{selected.title}.log" class="flex-1" />
					<div class="text-muted-foreground flex shrink-0 items-center gap-2 text-xs">
						<span class="tabular">{duration(selected)}</span>
						{#if selected.exitCode !== null}<span>exit code {selected.exitCode}</span>{/if}
						{#if selected.error}<span class="text-destructive truncate">{selected.error.message}</span>{/if}
						{#if selected.running}
							<Button variant="destructive" size="xs" class="ml-auto" onclick={() => selected.stop()}>
								<Square /> Stop
							</Button>
						{/if}
					</div>
				</div>
			{:else}
				<div class="min-h-0 flex-1 overflow-y-auto">
					{#each list as job (job.id)}
						<div class="hover:bg-muted/50 flex items-center gap-2 border-b px-3 py-2 last:border-b-0">
							{@render statusIcon(job)}
							<button type="button" class="min-w-0 flex-1 text-left" onclick={() => (jobs.selectedId = job.id)}>
								<p class="truncate text-sm font-medium">{job.title}</p>
								{#if job.detail}
									<p class="text-muted-foreground truncate font-mono text-[11px]">{job.detail}</p>
								{/if}
							</button>
							<span class="text-muted-foreground tabular text-xs">{duration(job)}</span>
							{#if job.running}
								<IconButton label="Stop" onclick={() => job.stop()}><Square /></IconButton>
							{/if}
						</div>
					{:else}
						<p class="text-muted-foreground p-6 text-center text-sm">
							Long-running operations show up here.
						</p>
					{/each}
				</div>
				<footer class="flex shrink-0 items-center justify-end border-t px-2 py-1.5">
					<Button
						variant="ghost"
						size="xs"
						disabled={counts.done + counts.failed === 0}
						onclick={() => jobs.clearFinished()}
					>
						Clear finished
					</Button>
				</footer>
			{/if}
		{/if}
	</section>
{/if}
