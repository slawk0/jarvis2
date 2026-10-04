<!-- Refresh button with an optional auto-refresh toggle and interval. -->
<script lang="ts">
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import { Button } from '$lib/components/ui/button';
	import { Switch } from '$lib/components/ui/switch';
	import { cn } from '$lib/utils';
	import SelectField from './SelectField.svelte';
	import Tip from './Tip.svelte';

	interface Props {
		onrefresh: () => void;
		loading?: boolean;
		/** Bind to show the auto-refresh switch. */
		auto?: boolean;
		/** Bind (seconds) together with `intervals` to let the user pick. */
		interval?: number;
		intervals?: number[];
		label?: string;
	}

	let {
		onrefresh,
		loading = false,
		auto = $bindable(),
		interval = $bindable(),
		intervals,
		label = 'Refresh'
	}: Props = $props();
</script>

<div class="flex items-center gap-2">
	{#if auto !== undefined}
		<label class="flex items-center gap-1.5 text-xs text-muted-foreground">
			<Switch size="sm" bind:checked={auto} />
			Auto-refresh
		</label>
		{#if intervals && interval !== undefined && auto}
			<SelectField
				size="sm"
				class="w-20"
				value={String(interval)}
				options={intervals.map((s) => ({ value: String(s), label: `${s} s` }))}
				onchange={(v) => (interval = Number(v))}
			/>
		{/if}
	{/if}
	<Tip text={label}>
		<Button variant="outline" size="icon-sm" onclick={onrefresh} aria-label={label}>
			<RefreshCw class={cn(loading && 'animate-spin')} />
		</Button>
	</Tip>
</div>
