<!-- SVG world choropleth: countries shaded by request count, with a hover card. -->
<script lang="ts">
	import world from '@svg-maps/world';
	import { formatNumber } from '$lib/format';
	import { countryName } from './client';

	interface Props {
		countries: { code: string; count: number; blocked: number }[];
	}

	let { countries }: Props = $props();

	const byCode = $derived(new Map(countries.map((c) => [c.code.toLowerCase(), c])));
	const max = $derived(Math.max(1, ...countries.map((c) => c.count)));
	let hover = $state<{ code: string; x: number; y: number } | null>(null);
	let box = $state<HTMLDivElement | null>(null);

	/** Log scale so one busy country does not flatten all the others. */
	function shade(count: number): string {
		const strength = 0.18 + 0.82 * (Math.log1p(count) / Math.log1p(max));
		return `color-mix(in oklab, var(--primary) ${Math.round(strength * 100)}%, var(--muted))`;
	}

	function move(event: MouseEvent, code: string): void {
		const rect = box?.getBoundingClientRect();
		if (rect) hover = { code, x: event.clientX - rect.left, y: event.clientY - rect.top };
	}

	const hovered = $derived(
		hover ? (byCode.get(hover.code) ?? { code: hover.code, count: 0, blocked: 0 }) : null
	);
</script>

<div
	bind:this={box}
	class="relative"
	role="img"
	aria-label="Requests by country"
	onmouseleave={() => (hover = null)}
>
	<svg viewBox={world.viewBox} class="h-auto w-full">
		{#each world.locations as location (location.id)}
			{@const data = byCode.get(location.id)}
			<path
				d={location.path}
				fill={data ? shade(data.count) : 'var(--muted)'}
				stroke="var(--card)"
				stroke-width="0.5"
				class="hover:brightness-125"
				role="presentation"
				onmousemove={(e) => move(e, location.id)}
			/>
		{/each}
	</svg>
	{#if hover && hovered}
		<div
			class="pointer-events-none absolute z-10 rounded-md border bg-popover px-2.5 py-1.5 text-xs text-popover-foreground shadow-md"
			style="left: {Math.min(hover.x + 12, (box?.clientWidth ?? 0) - 170)}px; top: {hover.y + 12}px"
		>
			<p class="font-medium">{countryName(hover.code)}</p>
			<p class="text-muted-foreground tabular">
				{formatNumber(hovered.count)} request{hovered.count === 1 ? '' : 's'}{hovered.blocked
					? ` · ${formatNumber(hovered.blocked)} blocked`
					: ''}
			</p>
		</div>
	{/if}
</div>
