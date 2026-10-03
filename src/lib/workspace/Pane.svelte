<!-- One pane: optional header (when there are several panes) and its kept-alive tabs. -->
<script lang="ts">
	import Columns2 from '@lucide/svelte/icons/columns-2';
	import GripVertical from '@lucide/svelte/icons/grip-vertical';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Rows2 from '@lucide/svelte/icons/rows-2';
	import X from '@lucide/svelte/icons/x';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import IconButton from '$lib/components/IconButton.svelte';
	import { cn } from '$lib/utils';
	import { CATEGORIES, TABS, tabDef, type TabId } from './registry';
	import TabHost from './TabHost.svelte';
	import { providePaneContext, workspace } from './workspace.svelte';

	interface Props {
		paneId: string;
		/** Start dragging this pane by its header. */
		ondragstart: (event: PointerEvent) => void;
	}

	let { paneId, ondragstart }: Props = $props();

	// svelte-ignore state_referenced_locally
	providePaneContext(paneId);

	const pane = $derived(workspace.pane(paneId));
	const focused = $derived(workspace.focusedId === paneId);
	const multi = $derived(workspace.paneCount > 1);
	const current = $derived(pane ? tabDef(pane.tab) : undefined);
</script>

{#if pane}
	<div
		class={cn(
			'bg-background flex h-full min-h-0 flex-col overflow-hidden',
			multi && 'rounded-lg border',
			multi && focused && 'border-primary/60 ring-primary/20 ring-1'
		)}
		onpointerdowncapture={() => workspace.focus(paneId)}
		onfocusin={() => workspace.focus(paneId)}
	>
		{#if multi}
			<header class="bg-card flex h-8 shrink-0 items-center gap-1 border-b pr-1 pl-0.5">
				<button
					type="button"
					class="text-muted-foreground hover:text-foreground flex h-6 w-5 cursor-grab items-center justify-center active:cursor-grabbing"
					aria-label="Drag to move pane"
					onpointerdown={ondragstart}
				>
					<GripVertical class="size-3.5" />
				</button>
				<DropdownMenu.Root>
					<DropdownMenu.Trigger
						class="hover:bg-muted flex h-6 min-w-0 items-center gap-1.5 rounded px-1.5 text-xs font-medium"
					>
						{#if current}<current.icon class="size-3.5 shrink-0" />{/if}
						<span class="truncate">{current?.label}</span>
						<ChevronDown class="text-muted-foreground size-3 shrink-0" />
					</DropdownMenu.Trigger>
					<DropdownMenu.Content align="start" class="max-h-[70vh] w-52 overflow-y-auto">
						{#each CATEGORIES as category (category)}
							<DropdownMenu.Group>
								<DropdownMenu.GroupHeading class="text-muted-foreground text-[11px]">{category}</DropdownMenu.GroupHeading>
								{#each TABS.filter((t) => t.category === category) as t (t.id)}
									<DropdownMenu.Item onclick={() => workspace.openTab(t.id as TabId, paneId)}>
										<t.icon class="size-3.5" />
										{t.label}
									</DropdownMenu.Item>
								{/each}
							</DropdownMenu.Group>
						{/each}
					</DropdownMenu.Content>
				</DropdownMenu.Root>
				<span class="flex-1"></span>
				<IconButton label="Refresh" size="icon-xs" onclick={() => workspace.refreshActive(paneId)}>
					<RefreshCw />
				</IconButton>
				<IconButton
					label="Split vertically"
					size="icon-xs"
					disabled={!workspace.canSplit}
					onclick={() => workspace.split('vertical', paneId)}
				>
					<Columns2 />
				</IconButton>
				<IconButton
					label="Split horizontally"
					size="icon-xs"
					disabled={!workspace.canSplit}
					onclick={() => workspace.split('horizontal', paneId)}
				>
					<Rows2 />
				</IconButton>
				<IconButton label="Close pane" size="icon-xs" onclick={() => workspace.close(paneId)}>
					<X />
				</IconButton>
			</header>
		{/if}
		<div class="relative min-h-0 flex-1">
			{#each pane.mounted as tabId (tabId)}
				{@const def = tabDef(tabId)}
				{#if def}
					<TabHost {paneId} {def} visible={pane.tab === tabId} />
				{/if}
			{/each}
		</div>
	</div>
{/if}
