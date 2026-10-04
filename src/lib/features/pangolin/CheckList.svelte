<!-- A compact multi-select: a scrollable list of checkboxes bound to an array of values. -->
<script lang="ts">
	import { Checkbox } from '$lib/components/ui/checkbox';

	interface Props {
		options: { value: string; label: string }[];
		selected: string[];
		empty?: string;
	}

	let { options, selected = $bindable(), empty = 'Nothing to choose from' }: Props = $props();

	function toggle(value: string, on: boolean): void {
		selected = on ? [...selected.filter((v) => v !== value), value] : selected.filter((v) => v !== value);
	}
</script>

<div class="max-h-36 overflow-y-auto rounded-md border bg-card px-2 py-1.5">
	{#each options as option (option.value)}
		<label class="flex items-center gap-2 py-0.5 text-sm">
			<Checkbox
				checked={selected.includes(option.value)}
				onCheckedChange={(v) => toggle(option.value, v === true)}
			/>
			<span class="truncate" title={option.label}>{option.label}</span>
		</label>
	{:else}
		<p class="text-xs text-muted-foreground">{empty}</p>
	{/each}
</div>
