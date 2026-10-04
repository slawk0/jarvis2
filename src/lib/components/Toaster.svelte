<script lang="ts">
	import { fly } from 'svelte/transition';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import Copy from '@lucide/svelte/icons/copy';
	import Info from '@lucide/svelte/icons/info';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import X from '@lucide/svelte/icons/x';
	import { toast, type ToastKind } from '$lib/services/toast.svelte';
	import { cn, copyText } from '$lib/utils';

	const ICONS = { success: CircleCheck, info: Info, warning: TriangleAlert, error: CircleAlert };
	const TONE: Record<ToastKind, string> = {
		success: 'text-success',
		info: 'text-info',
		warning: 'text-warning',
		error: 'text-destructive'
	};
</script>

<div
	class="pointer-events-none fixed right-4 bottom-4 z-[100] flex w-96 max-w-[calc(100vw-2rem)] flex-col gap-2"
>
	{#each toast.items as item (item.id)}
		{@const Icon = ICONS[item.kind]}
		<div
			role={item.kind === 'error' ? 'alert' : 'status'}
			class="pointer-events-auto flex items-start gap-2.5 rounded-lg border bg-popover p-3 text-popover-foreground shadow-lg"
			transition:fly={{ x: 24, duration: 160 }}
			onpointerenter={() => toast.hold(item.id)}
			onpointerleave={() => toast.release(item.id)}
		>
			<Icon class={cn('mt-0.5 size-4 shrink-0', TONE[item.kind])} />
			<div class="selectable min-w-0 flex-1">
				<p class="text-sm leading-snug font-medium break-words">{item.message}</p>
				{#if item.detail}
					<p
						class="mt-1 max-h-32 overflow-auto text-xs break-words whitespace-pre-wrap text-muted-foreground"
					>
						{item.detail}
					</p>
				{/if}
			</div>
			<button
				type="button"
				class="rounded p-0.5 text-muted-foreground hover:text-foreground"
				aria-label="Copy message"
				onclick={() => copyText(item.detail ? `${item.message}\n${item.detail}` : item.message)}
			>
				<Copy class="size-3.5" />
			</button>
			<button
				type="button"
				class="rounded p-0.5 text-muted-foreground hover:text-foreground"
				aria-label="Dismiss"
				onclick={() => toast.dismiss(item.id)}
			>
				<X class="size-3.5" />
			</button>
		</div>
	{/each}
</div>
