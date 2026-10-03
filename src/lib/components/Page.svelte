<!--
	Standard tab layout: a toolbar row (title + controls) and a body that
	fills the pane. Keeps spacing and titles identical across tabs.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils';

	interface Props {
		title?: string;
		subtitle?: string;
		/** Controls on the right of the title. */
		toolbar?: Snippet;
		/** Row under the title (sub-tabs, filters). */
		header?: Snippet;
		children: Snippet;
		/** Body scrolls by default; set false when the content manages its own scrolling. */
		scroll?: boolean;
		class?: string;
	}

	let { title, subtitle, toolbar, header, children, scroll = true, class: className }: Props = $props();
</script>

<div class="flex h-full min-h-0 flex-col">
	{#if title || toolbar}
		<div class="flex min-h-11 shrink-0 flex-wrap items-center gap-x-3 gap-y-1.5 px-4 py-2">
			{#if title}
				<div class="mr-auto min-w-0">
					<h2 class="truncate text-[15px] leading-tight font-semibold">{title}</h2>
					{#if subtitle}<p class="text-muted-foreground truncate text-xs">{subtitle}</p>{/if}
				</div>
			{/if}
			{#if toolbar}
				<div class={cn('flex flex-wrap items-center gap-2', !title && 'w-full')}>{@render toolbar()}</div>
			{/if}
		</div>
	{/if}
	{#if header}
		<div class="shrink-0 px-4">{@render header()}</div>
	{/if}
	<div class={cn('min-h-0 flex-1 p-4', scroll ? 'overflow-auto' : 'flex flex-col overflow-hidden', className)}>
		{@render children()}
	</div>
</div>
