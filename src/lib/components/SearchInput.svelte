<!-- Search box with a clear button. -->
<script lang="ts">
	import Search from '@lucide/svelte/icons/search';
	import X from '@lucide/svelte/icons/x';
	import { Input } from '$lib/components/ui/input';
	import { cn } from '$lib/utils';

	interface Props {
		value: string;
		placeholder?: string;
		class?: string;
		ref?: HTMLInputElement | null;
	}

	let { value = $bindable(''), placeholder = 'Search…', class: className, ref = $bindable(null) }: Props = $props();
</script>

<div class={cn('relative w-56', className)}>
	<Search class="text-muted-foreground pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2" />
	<Input
		bind:ref
		bind:value
		{placeholder}
		class="h-7 pr-7 pl-8"
		spellcheck="false"
		autocomplete="off"
		onkeydown={(e) => {
			if (e.key === 'Escape' && value) {
				e.stopPropagation();
				value = '';
			}
		}}
	/>
	{#if value}
		<button
			type="button"
			class="text-muted-foreground hover:text-foreground absolute top-1/2 right-1.5 -translate-y-1/2 rounded p-0.5"
			aria-label="Clear search"
			onclick={() => (value = '')}
		>
			<X class="size-3.5" />
		</button>
	{/if}
</div>
