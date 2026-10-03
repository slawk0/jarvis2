<!--
	Renders the pane tree. Panes are laid out flat (absolutely positioned from
	the computed rects) and keyed by id, so changing the layout never remounts
	a pane and its terminals/editors survive splits, moves and resizes.
-->
<script lang="ts">
	import { cn } from '$lib/utils';
	import { drag } from './drag.svelte';
	import { computeDividers, computeRects, type Divider, type Rect } from './layout';
	import Pane from './Pane.svelte';
	import { tabDef } from './registry';
	import { workspace } from './workspace.svelte';

	const MIN_PANE_PX = 260;

	let grid = $state<HTMLDivElement | null>(null);
	const rects = $derived(computeRects(workspace.layout));
	const dividers = $derived(computeDividers(workspace.layout));
	const multi = $derived(workspace.paneCount > 1);
	let resizing = $state<string | null>(null);

	$effect(() => {
		drag.grid = grid;
		return () => {
			drag.grid = null;
		};
	});

	const pct = (n: number) => `${(n * 100).toFixed(4)}%`;
	const box = (r: Rect) => `left:${pct(r.x)};top:${pct(r.y)};width:${pct(r.w)};height:${pct(r.h)}`;

	function startResize(event: PointerEvent, divider: Divider): void {
		if (!grid) return;
		event.preventDefault();
		const bounds = grid.getBoundingClientRect();
		const horizontal = divider.dir === 'row';
		const start = horizontal
			? bounds.left + divider.area.x * bounds.width
			: bounds.top + divider.area.y * bounds.height;
		const size = horizontal ? divider.area.w * bounds.width : divider.area.h * bounds.height;
		const min = Math.min(0.45, MIN_PANE_PX / size);
		resizing = divider.splitId;

		const move = (e: PointerEvent) => {
			const position = horizontal ? e.clientX : e.clientY;
			workspace.resize(divider.splitId, (position - start) / size, min);
		};
		const up = () => {
			resizing = null;
			window.removeEventListener('pointermove', move);
			window.removeEventListener('pointerup', up);
			document.body.style.cursor = '';
		};
		document.body.style.cursor = horizontal ? 'col-resize' : 'row-resize';
		window.addEventListener('pointermove', move);
		window.addEventListener('pointerup', up);
	}

	function startPaneDrag(event: PointerEvent, paneId: string): void {
		const pane = workspace.pane(paneId);
		drag.begin(event, { kind: 'pane', paneId }, tabDef(pane?.tab ?? '')?.label ?? 'Pane');
	}

	/** Highlighted region inside the target pane for the current drop zone. */
	function zoneBox(rect: Rect, zone: string): Rect {
		switch (zone) {
			case 'left':
				return { ...rect, w: rect.w / 2 };
			case 'right':
				return { ...rect, x: rect.x + rect.w / 2, w: rect.w / 2 };
			case 'top':
				return { ...rect, h: rect.h / 2 };
			case 'bottom':
				return { ...rect, y: rect.y + rect.h / 2, h: rect.h / 2 };
			default:
				return rect;
		}
	}

	const dropRect = $derived.by(() => {
		const target = drag.target;
		const rect = target && rects.get(target.paneId);
		return rect && target ? zoneBox(rect, target.zone) : null;
	});
	const dropHint = $derived.by(() => {
		if (!drag.payload || !drag.target) return '';
		if (drag.target.zone === 'center') return drag.payload.kind === 'tab' ? 'Open here' : 'Swap';
		return drag.payload.kind === 'tab' ? 'Open in new pane' : 'Move here';
	});
</script>

<div bind:this={grid} class={cn('relative h-full min-h-0 w-full', multi && 'bg-sunken')}>
	{#each Object.keys(workspace.panes) as paneId (paneId)}
		{@const rect = rects.get(paneId)}
		{#if rect}
			<div class={cn('absolute', multi && 'p-[3px]')} style={box(rect)}>
				<Pane {paneId} ondragstart={(e) => startPaneDrag(e, paneId)} />
			</div>
		{/if}
	{/each}

	{#each dividers as divider (divider.splitId)}
		<div
			role="separator"
			aria-orientation={divider.dir === 'row' ? 'vertical' : 'horizontal'}
			class={cn(
				'group absolute z-20',
				divider.dir === 'row' ? '-ml-[4px] w-[8px] cursor-col-resize' : '-mt-[4px] h-[8px] cursor-row-resize'
			)}
			style={divider.dir === 'row'
				? `left:${pct(divider.rect.x)};top:${pct(divider.rect.y)};height:${pct(divider.rect.h)}`
				: `left:${pct(divider.rect.x)};top:${pct(divider.rect.y)};width:${pct(divider.rect.w)}`}
			onpointerdown={(e) => startResize(e, divider)}
		>
			<div
				class={cn(
					'bg-primary absolute rounded-full opacity-0 transition-opacity group-hover:opacity-70',
					resizing === divider.splitId && 'opacity-100',
					divider.dir === 'row' ? 'inset-y-2 left-[3px] w-[2px]' : 'inset-x-2 top-[3px] h-[2px]'
				)}
			></div>
		</div>
	{/each}

	{#if drag.payload}
		<!-- Shield so panes do not react to the pointer while dragging. -->
		<div class="absolute inset-0 z-30"></div>
		{#if dropRect}
			<div
				class="border-primary bg-primary/15 pointer-events-none absolute z-30 flex items-center justify-center rounded-lg border-2 border-dashed transition-all duration-75"
				style={box(dropRect)}
			>
				<span class="bg-primary text-primary-foreground rounded-md px-2 py-1 text-xs font-medium shadow">
					{dropHint}
				</span>
			</div>
		{/if}
	{/if}
</div>

{#if drag.payload}
	<div
		class="bg-popover pointer-events-none fixed z-[90] rounded-md border px-2.5 py-1.5 text-xs font-medium shadow-lg"
		style="left:{drag.x + 12}px;top:{drag.y + 12}px"
	>
		{drag.label}
	</div>
{/if}
