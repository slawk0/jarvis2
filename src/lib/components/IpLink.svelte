<!-- An IP address with the shared context menu ("Look up in Net Diagnostics"). -->
<script lang="ts">
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { toast } from '$lib/services/toast.svelte';
	import { cn, copyText } from '$lib/utils';
	import { workspace } from '$lib/workspace/workspace.svelte';

	interface Props {
		ip: string | null | undefined;
		class?: string;
	}

	let { ip, class: className }: Props = $props();

	/** Strip a port, brackets or CIDR suffix so the lookup gets a bare address. */
	const bare = $derived.by(() => {
		const text = (ip ?? '').trim();
		const bracketed = /^\[([^\]]+)\]/.exec(text);
		if (bracketed) return bracketed[1];
		const noCidr = text.split('/')[0];
		return /^\d+\.\d+\.\d+\.\d+:\d+$/.test(noCidr) ? noCidr.split(':')[0] : noCidr;
	});
</script>

{#if ip}
	<ContextMenu.Root>
		<ContextMenu.Trigger>
			{#snippet child({ props })}
				<span {...props} class={cn('selectable font-mono', className)}>{ip}</span>
			{/snippet}
		</ContextMenu.Trigger>
		<ContextMenu.Content class="w-56">
			<ContextMenu.Item onclick={() => workspace.request('netdiag', { ip: bare })}>
				Look up in Net Diagnostics
			</ContextMenu.Item>
			<ContextMenu.Item onclick={() => copyText(bare).then(() => toast.success('IP copied'))}>
				Copy IP address
			</ContextMenu.Item>
		</ContextMenu.Content>
	</ContextMenu.Root>
{:else}
	<span class="text-muted-foreground">—</span>
{/if}
