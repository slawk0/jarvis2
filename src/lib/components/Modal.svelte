<!--
	The one modal. Esc, backdrop click and Back all close it (unless it is
	not dismissible, e.g. while an install is running).
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import { workspace } from '$lib/workspace/workspace.svelte';
	import { cn } from '$lib/utils';

	interface Props {
		open: boolean;
		title: string;
		description?: string;
		size?: 'sm' | 'md' | 'lg' | 'xl' | 'full';
		dismissible?: boolean;
		/** Remove the body padding/scroll so the content can manage its own layout. */
		flush?: boolean;
		children: Snippet;
		footer?: Snippet;
		onclose?: () => void;
		class?: string;
	}

	let {
		open = $bindable(false),
		title,
		description,
		size = 'md',
		dismissible = true,
		flush = false,
		children,
		footer,
		onclose,
		class: className
	}: Props = $props();

	const SIZES = {
		sm: 'sm:max-w-sm',
		md: 'sm:max-w-lg',
		lg: 'sm:max-w-2xl',
		xl: 'sm:max-w-4xl',
		full: 'sm:max-w-[min(1400px,94vw)] h-[90vh]'
	};

	$effect(() => {
		if (!open || !dismissible) return;
		return workspace.pushBack(`Close “${title}”`, () => {
			open = false;
			onclose?.();
		});
	});
</script>

<Dialog.Root
	bind:open
	onOpenChange={(value) => {
		if (!value) onclose?.();
	}}
>
	<Dialog.Content
		class={cn('flex max-h-[90vh] flex-col', SIZES[size], className)}
		showCloseButton={dismissible}
		interactOutsideBehavior={dismissible ? 'close' : 'ignore'}
		escapeKeydownBehavior={dismissible ? 'close' : 'ignore'}
	>
		<Dialog.Header>
			<Dialog.Title class="pr-8 text-base font-semibold">{title}</Dialog.Title>
			{#if description}
				<Dialog.Description class="text-muted-foreground">{description}</Dialog.Description>
			{/if}
		</Dialog.Header>
		<div class={cn('min-h-0 flex-1', flush ? 'flex flex-col' : '-mx-4 overflow-y-auto px-4 py-1')}>
			{@render children()}
		</div>
		{#if footer}
			<Dialog.Footer>{@render footer()}</Dialog.Footer>
		{/if}
	</Dialog.Content>
</Dialog.Root>
