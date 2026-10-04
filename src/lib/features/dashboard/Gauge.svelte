<!-- Ring gauge for a percentage. -->
<script lang="ts">
	interface Props {
		label: string;
		percent: number | null;
		detail?: string;
		/** Percent from which the ring turns amber / red. */
		warn?: number;
		danger?: number;
	}

	let { label, percent, detail, warn = 70, danger = 90 }: Props = $props();

	const R = 42;
	const C = 2 * Math.PI * R;
	const value = $derived(Math.min(100, Math.max(0, percent ?? 0)));
	const tone = $derived(
		value >= danger ? 'var(--destructive)' : value >= warn ? 'var(--warning)' : 'var(--primary)'
	);
</script>

<div class="flex items-center gap-4">
	<svg
		viewBox="0 0 100 100"
		class="size-24 shrink-0 -rotate-90"
		role="img"
		aria-label="{label} {Math.round(value)}%"
	>
		<circle cx="50" cy="50" r={R} fill="none" stroke="var(--muted)" stroke-width="9" />
		<circle
			cx="50"
			cy="50"
			r={R}
			fill="none"
			stroke={tone}
			stroke-width="9"
			stroke-linecap="round"
			stroke-dasharray={C}
			stroke-dashoffset={C * (1 - value / 100)}
			style="transition: stroke-dashoffset 0.5s ease, stroke 0.3s"
		/>
	</svg>
	<div class="min-w-0">
		<p class="text-xs font-medium text-muted-foreground">{label}</p>
		<p class="text-2xl leading-tight font-semibold tabular">
			{percent == null ? '—' : `${Math.round(value)}%`}
		</p>
		{#if detail}<p class="truncate text-xs text-muted-foreground tabular">{detail}</p>{/if}
	</div>
</div>
