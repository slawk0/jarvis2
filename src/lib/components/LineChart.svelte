<!-- Small dependency-free area/line chart for time series. -->
<script lang="ts">
	interface Series {
		label: string;
		color: string;
		values: number[];
	}

	interface Props {
		series: Series[];
		/** Fixed upper bound (e.g. 100 for percentages); auto-scaled when omitted. */
		max?: number;
		/** Number of slots on the x axis (the rolling window size). */
		slots?: number;
		height?: number;
		format?: (value: number) => string;
		labels?: string[];
	}

	let { series, max, slots, height = 140, format = (v) => String(Math.round(v)), labels }: Props = $props();

	const W = 600;
	const count = $derived(slots ?? Math.max(2, ...series.map((s) => s.values.length)));
	const top = $derived(max ?? Math.max(1, ...series.flatMap((s) => s.values)) * 1.1);

	function points(values: number[]): [number, number][] {
		const offset = count - values.length;
		return values.map((v, i) => [
			((i + offset) / (count - 1)) * W,
			height - (Math.min(v, top) / top) * (height - 4) - 2
		]);
	}

	const line = (pts: [number, number][]) =>
		pts.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
	const area = (pts: [number, number][]) =>
		pts.length < 2
			? ''
			: `${line(pts)} L${pts[pts.length - 1][0].toFixed(1)},${height} L${pts[0][0].toFixed(1)},${height} Z`;
</script>

<div>
	<div class="relative">
		<svg
			viewBox="0 0 {W} {height}"
			preserveAspectRatio="none"
			class="w-full"
			style="height: {height}px"
			role="img"
		>
			{#each [0.25, 0.5, 0.75] as f (f)}
				<line
					x1="0"
					x2={W}
					y1={height * f}
					y2={height * f}
					stroke="var(--border)"
					stroke-width="1"
					vector-effect="non-scaling-stroke"
				/>
			{/each}
			{#each series as s (s.label)}
				{@const pts = points(s.values)}
				<path d={area(pts)} fill={s.color} opacity="0.12" />
				<path
					d={line(pts)}
					fill="none"
					stroke={s.color}
					stroke-width="1.75"
					vector-effect="non-scaling-stroke"
					stroke-linejoin="round"
				/>
			{/each}
		</svg>
		<span class="absolute top-0 left-0 text-[10px] text-muted-foreground tabular">{format(top)}</span>
	</div>
	{#if labels}
		<div class="mt-1 flex justify-between text-[10px] text-muted-foreground tabular">
			{#each labels as label (label)}<span>{label}</span>{/each}
		</div>
	{/if}
	<div class="mt-1.5 flex flex-wrap gap-3 text-xs">
		{#each series as s (s.label)}
			<span class="flex items-center gap-1.5 text-muted-foreground">
				<span class="size-2 rounded-full" style="background: {s.color}"></span>
				{s.label}
				{#if s.values.length}<span class="text-foreground tabular"
						>{format(s.values[s.values.length - 1])}</span
					>{/if}
			</span>
		{/each}
	</div>
</div>
