<!-- The sub-tab bar used inside feature tabs. -->
<script lang="ts" generics="T extends string">
	import type { Component } from 'svelte';
	import { cn } from '$lib/utils';

	interface Item {
		id: T;
		label: string;
		icon?: Component<{ class?: string }>;
		count?: number | null;
	}

	interface Props {
		items: readonly Item[];
		value: T;
		class?: string;
		onchange?: (value: T) => void;
	}

	let { items, value = $bindable(), class: className, onchange }: Props = $props();
</script>

<div role="tablist" class={cn('flex items-center gap-0.5 border-b', className)}>
	{#each items as item (item.id)}
		{@const active = item.id === value}
		<button
			type="button"
			role="tab"
			aria-selected={active}
			class={cn(
				'relative -mb-px flex h-8 items-center gap-1.5 border-b-2 px-3 text-sm whitespace-nowrap transition-colors',
				active
					? 'border-primary font-medium text-foreground'
					: 'border-transparent text-muted-foreground hover:text-foreground'
			)}
			onclick={() => {
				value = item.id;
				onchange?.(item.id);
			}}
		>
			{#if item.icon}<item.icon class="size-3.5" />{/if}
			{item.label}
			{#if item.count != null}
				<span class="rounded-full bg-muted px-1.5 text-[11px] leading-4 text-muted-foreground tabular">
					{item.count}
				</span>
			{/if}
		</button>
	{/each}
</div>
