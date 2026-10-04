<script lang="ts">
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Columns2 from '@lucide/svelte/icons/columns-2';
	import Grid2x2 from '@lucide/svelte/icons/grid-2x2';
	import Keyboard from '@lucide/svelte/icons/keyboard';
	import ListTodo from '@lucide/svelte/icons/list-todo';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Rows2 from '@lucide/svelte/icons/rows-2';
	import Settings from '@lucide/svelte/icons/settings';
	import Square from '@lucide/svelte/icons/square';
	import IconButton from '$lib/components/IconButton.svelte';
	import { jobs } from '$lib/services/jobs.svelte';
	import { tabDef } from './registry';
	import { workspace } from './workspace.svelte';

	const current = $derived(tabDef(workspace.focused?.tab ?? ''));
	const title = $derived(
		workspace.paneCount > 1 ? `Workspace · ${workspace.paneCount} panes` : (current?.label ?? '')
	);
	const backLabel = $derived(workspace.backLabel);
</script>

<header class="flex h-12 shrink-0 items-center gap-1 border-b bg-background px-3">
	<IconButton
		label={backLabel ?? 'Nothing to go back to'}
		side="bottom"
		disabled={!backLabel}
		onclick={() => workspace.back()}
	>
		<ArrowLeft />
	</IconButton>
	{#if workspace.paneCount === 1 && current}
		<current.icon class="ml-1 size-4 text-primary" />
	{/if}
	<h1 class="ml-1 truncate text-[15px] font-semibold">{title}</h1>

	<div class="ml-auto flex items-center gap-0.5">
		<IconButton label="Refresh active tab" side="bottom" onclick={() => workspace.refreshActive()}>
			<RefreshCw />
		</IconButton>
		<div class="relative">
			<IconButton
				label="Running jobs"
				side="bottom"
				variant={jobs.panelOpen ? 'secondary' : 'ghost'}
				onclick={() => (jobs.panelOpen = !jobs.panelOpen)}
			>
				<ListTodo />
			</IconButton>
			{#if jobs.runningCount > 0}
				<span
					class="pointer-events-none absolute -top-0.5 -right-0.5 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-info px-0.5 text-[9px] font-bold text-background tabular"
				>
					{jobs.runningCount}
				</span>
			{/if}
		</div>
		<span class="mx-1.5 h-5 w-px bg-border"></span>
		<IconButton label="Single pane" side="bottom" onclick={() => workspace.applyPreset('single')}>
			<Square />
		</IconButton>
		<IconButton label="Side by side" side="bottom" onclick={() => workspace.applyPreset('columns')}>
			<Columns2 />
		</IconButton>
		<IconButton label="Stacked" side="bottom" onclick={() => workspace.applyPreset('rows')}>
			<Rows2 />
		</IconButton>
		<IconButton label="2 × 2 grid" side="bottom" onclick={() => workspace.applyPreset('grid')}>
			<Grid2x2 />
		</IconButton>
		<span class="mx-1.5 h-5 w-px bg-border"></span>
		<IconButton
			label="Keyboard shortcuts (Ctrl+Shift+H)"
			side="bottom"
			onclick={() => (workspace.shortcutsOpen = true)}
		>
			<Keyboard />
		</IconButton>
		<IconButton label="Settings" side="bottom" onclick={() => workspace.openSettings()}>
			<Settings />
		</IconButton>
	</div>
</header>
