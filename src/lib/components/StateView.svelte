<!--
	The shared loading / error / empty placeholder, so every tab presents
	these states the same way.
-->
<script lang="ts">
	import type { Component, Snippet } from 'svelte';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Inbox from '@lucide/svelte/icons/inbox';
	import { Button } from '$lib/components/ui/button';
	import type { IpcError } from '$lib/ipc';
	import { cn } from '$lib/utils';

	interface Props {
		kind: 'loading' | 'error' | 'empty';
		title?: string;
		message?: string;
		error?: IpcError | null;
		icon?: Component<{ class?: string }>;
		onretry?: () => void;
		compact?: boolean;
		class?: string;
		/** Extra actions under the message. */
		children?: Snippet;
	}

	let {
		kind,
		title,
		message,
		error,
		icon,
		onretry,
		compact = false,
		class: className,
		children
	}: Props = $props();

	const Icon = $derived(icon ?? (kind === 'error' ? CircleAlert : Inbox));
	const needsRoot = $derived(error?.is('SUDO_PASSWORD_REQUIRED', 'SUDO_PASSWORD_EXPIRED') ?? false);
</script>

<div
	class={cn(
		'flex flex-col items-center justify-center gap-2 text-center text-muted-foreground',
		compact ? 'p-4' : 'h-full min-h-40 p-8',
		className
	)}
>
	{#if kind === 'loading'}
		<LoaderCircle class="size-5 animate-spin" />
		<p class="text-sm">{title ?? 'Loading…'}</p>
	{:else}
		<Icon class={cn('size-7', kind === 'error' ? 'text-destructive' : 'opacity-60')} />
		<p class="text-sm font-medium text-foreground">
			{title ?? (kind === 'error' ? (error?.title ?? 'Something went wrong') : 'Nothing here yet')}
		</p>
		{#if message ?? error?.details}
			<p class="selectable max-w-xl text-xs break-words whitespace-pre-wrap">
				{message ?? error?.details}
			</p>
		{/if}
		{#if children || onretry}
			<div class="mt-1 flex items-center gap-2">
				{@render children?.()}
				{#if onretry}
					<Button variant="outline" size="sm" onclick={onretry}>
						{needsRoot ? 'Authenticate' : 'Retry'}
					</Button>
				{/if}
			</div>
		{/if}
	{/if}
</div>
