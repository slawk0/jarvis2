<!-- Modal that shows a job's live output; the same job is listed in Running Jobs. -->
<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import type { Job } from '$lib/services/jobs.svelte';
	import LogViewer from './LogViewer.svelte';
	import Modal from './Modal.svelte';

	interface Props {
		job: Job | null;
		title?: string;
		onclose?: () => void;
	}

	let { job = $bindable(null), title, onclose }: Props = $props();

	function close(): void {
		job = null;
		onclose?.();
	}
</script>

{#if job}
	{@const current = job}
	<Modal open title={title ?? current.title} size="xl" class="h-[70vh]" flush onclose={close}>
		<div class="flex min-h-0 flex-1 flex-col gap-2">
			<LogViewer source={current} downloadName="{current.title}.log" class="flex-1" />
			{#if current.error}
				<p class="selectable text-xs text-destructive" role="alert">{current.error.message}</p>
			{/if}
		</div>
		{#snippet footer()}
			<span class="mr-auto self-center text-xs text-muted-foreground" role="status">
				{#if current.running}
					Running… you can close this window; it continues in Running Jobs.
				{:else if current.status === 'done'}
					<span class="text-success">Finished successfully.</span>
				{:else if current.status === 'cancelled'}
					Stopped.
				{:else}
					<span class="text-destructive"
						>Failed{current.exitCode !== null ? ` (exit code ${current.exitCode})` : ''}.</span
					>
				{/if}
			</span>
			{#if current.running}
				<Button variant="destructive" onclick={() => current.stop()}>Stop</Button>
			{/if}
			<Button variant="outline" onclick={close}>Close</Button>
		{/snippet}
	</Modal>
{/if}
