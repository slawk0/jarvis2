<!-- Tooltip around any element. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import * as Tooltip from '$lib/components/ui/tooltip';

	interface Props {
		text: string | null | undefined;
		side?: 'top' | 'right' | 'bottom' | 'left';
		children: Snippet;
		class?: string;
	}

	let { text, side = 'top', children, class: className = 'inline-flex' }: Props = $props();
</script>

{#if text}
	<Tooltip.Root>
		<Tooltip.Trigger>
			{#snippet child({ props })}
				<span {...props} class={className}>{@render children()}</span>
			{/snippet}
		</Tooltip.Trigger>
		<Tooltip.Content {side}>{text}</Tooltip.Content>
	</Tooltip.Root>
{:else}
	<span class={className}>{@render children()}</span>
{/if}
