<!-- Label + control + hint/error, for forms. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Label } from '$lib/components/ui/label';
	import { cn } from '$lib/utils';

	interface Props {
		label: string;
		for?: string;
		hint?: string;
		error?: string | null;
		required?: boolean;
		class?: string;
		children: Snippet;
	}

	let { label, for: htmlFor, hint, error, required = false, class: className, children }: Props = $props();
</script>

<div class={cn('flex flex-col gap-1.5', className)}>
	<Label for={htmlFor} class="text-xs font-medium text-muted-foreground">
		{label}{#if required}<span class="text-destructive"> *</span>{/if}
	</Label>
	{@render children()}
	{#if error}
		<p class="text-xs text-destructive">{error}</p>
	{:else if hint}
		<p class="text-xs text-muted-foreground">{hint}</p>
	{/if}
</div>
