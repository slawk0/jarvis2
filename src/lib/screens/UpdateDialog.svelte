<script lang="ts">
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Progress } from '$lib/components/ui/progress';
	import Modal from '$lib/components/Modal.svelte';
	import { formatBytes } from '$lib/format';
	import { updater } from '$lib/services/updater.svelte';

	onMount(() => {
		// Quietly look for an update shortly after start.
		const timer = setTimeout(() => void updater.check(true), 3000);
		return () => clearTimeout(timer);
	});
</script>

{#if updater.open}
	<Modal
		open
		title="Update available"
		description="Jarvis {updater.version} is ready to install."
		size="md"
		dismissible={updater.dismissible}
		onclose={() => updater.later()}
	>
		<div class="flex flex-col gap-3">
			{#if updater.notes}
				<!-- Release notes are rendered as plain text, never as HTML. -->
				<pre
					class="selectable max-h-64 overflow-auto rounded-md border bg-sunken p-3 font-sans text-xs whitespace-pre-wrap">{updater.notes}</pre>
			{/if}
			{#if updater.phase === 'downloading' || updater.phase === 'installing'}
				<div class="flex flex-col gap-1.5">
					<Progress value={updater.progress ?? 0} class="h-1.5" />
					<p class="text-xs text-muted-foreground tabular">
						{#if updater.phase === 'installing'}
							Installing… Jarvis will restart.
						{:else if updater.total}
							Downloading {formatBytes(updater.downloaded)} of {formatBytes(updater.total)}
						{:else}
							Downloading {formatBytes(updater.downloaded)}
						{/if}
					</p>
				</div>
			{/if}
			{#if updater.phase === 'error'}
				<p class="selectable text-xs text-destructive" role="alert">The update failed: {updater.error}</p>
			{/if}
		</div>
		{#snippet footer()}
			{#if updater.phase === 'error'}
				<Button variant="outline" onclick={() => updater.later()}>Close</Button>
				<Button onclick={() => updater.install()}>Retry</Button>
			{:else}
				<Button variant="outline" disabled={!updater.dismissible} onclick={() => updater.later()}
					>Later</Button
				>
				<Button disabled={!updater.dismissible} onclick={() => updater.install()}>
					{updater.dismissible ? 'Update now' : 'Updating…'}
				</Button>
			{/if}
		{/snippet}
	</Modal>
{/if}
