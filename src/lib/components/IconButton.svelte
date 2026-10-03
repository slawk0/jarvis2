<!-- Icon-only button with a tooltip that doubles as its accessible name. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { Button, type ButtonVariant } from '$lib/components/ui/button';
	import Tip from './Tip.svelte';

	interface Props {
		label: string;
		onclick?: (event: MouseEvent) => void;
		disabled?: boolean;
		busy?: boolean;
		variant?: ButtonVariant;
		size?: 'icon-xs' | 'icon-sm' | 'icon';
		side?: 'top' | 'right' | 'bottom' | 'left';
		class?: string;
		children: Snippet;
	}

	let {
		label,
		onclick,
		disabled = false,
		busy = false,
		variant = 'ghost',
		size = 'icon-sm',
		side = 'top',
		class: className,
		children
	}: Props = $props();
</script>

<Tip text={label} {side}>
	<Button {variant} {size} class={className} disabled={disabled || busy} aria-label={label} {onclick}>
		{#if busy}
			<LoaderCircle class="animate-spin" />
		{:else}
			{@render children()}
		{/if}
	</Button>
</Tip>
