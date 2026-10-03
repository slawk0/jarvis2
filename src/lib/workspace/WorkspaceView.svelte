<!-- The connected view: sidebar, workspace bar and the pane grid. -->
<script lang="ts">
	import { onMount } from 'svelte';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import Power from '@lucide/svelte/icons/power';
	import { Button } from '$lib/components/ui/button';
	import Modal from '$lib/components/Modal.svelte';
	import { app } from '$lib/services/app.svelte';
	import PaneGrid from './PaneGrid.svelte';
	import { SHORTCUTS, handleShortcut } from './shortcuts';
	import Sidebar from './Sidebar.svelte';
	import { workspace } from './workspace.svelte';
	import WorkspaceBar from './WorkspaceBar.svelte';

	let ready = $state(false);

	onMount(() => {
		void workspace.load().finally(() => (ready = true));

		const onKey = (e: KeyboardEvent) => handleShortcut(e);
		// Mouse button 4 ("back"). Handled on mouseup so it fires once per press.
		const onMouseUp = (e: MouseEvent) => {
			if (e.button === 3) {
				e.preventDefault();
				workspace.back();
			}
		};
		const swallowBack = (e: MouseEvent) => {
			if (e.button === 3 || e.button === 4) e.preventDefault();
		};
		window.addEventListener('keydown', onKey, true);
		window.addEventListener('mouseup', onMouseUp);
		window.addEventListener('mousedown', swallowBack);
		return () => {
			window.removeEventListener('keydown', onKey, true);
			window.removeEventListener('mouseup', onMouseUp);
			window.removeEventListener('mousedown', swallowBack);
		};
	});
</script>

<div class="flex h-full min-h-0">
	<Sidebar />
	<div class="flex min-w-0 flex-1 flex-col">
		<WorkspaceBar />
		{#if app.status === 'offline' || app.status === 'reconnecting'}
			<div
				class="bg-destructive/12 text-destructive flex shrink-0 items-center gap-2 border-b px-4 py-1.5 text-xs"
				role="status"
			>
				<WifiOff class="size-3.5" />
				<span>
					Connection lost.
					{app.status === 'reconnecting'
						? `Reconnecting (attempt ${app.reconnectAttempt})…`
						: 'Waiting to retry…'}
				</span>
				<Button variant="outline" size="xs" class="ml-auto" onclick={() => app.reconnectNow()}>
					Reconnect now
				</Button>
			</div>
		{/if}
		<main class="relative min-h-0 flex-1">
			{#if ready}
				<PaneGrid />
			{/if}
			{#if app.rebooting}
				<div class="bg-background/90 absolute inset-0 z-40 flex flex-col items-center justify-center gap-3 backdrop-blur-sm">
					<Power class="text-warning size-10 animate-pulse" />
					<p class="text-base font-semibold">The server is rebooting</p>
					<p class="text-muted-foreground text-sm">
						Jarvis reconnects automatically once it is back
						{#if app.reconnectAttempt > 0}(attempt {app.reconnectAttempt}){/if}.
					</p>
					<div class="flex gap-2">
						<Button variant="outline" size="sm" onclick={() => app.reconnectNow()}>Try now</Button>
						<Button variant="ghost" size="sm" onclick={() => app.disconnect()}>Disconnect</Button>
					</div>
				</div>
			{/if}
		</main>
	</div>
</div>

<Modal bind:open={workspace.shortcutsOpen} title="Keyboard shortcuts" size="md">
	<dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
		{#each SHORTCUTS as shortcut (shortcut.keys)}
			<dt>
				<kbd class="bg-muted rounded border px-1.5 py-0.5 text-xs whitespace-nowrap">{shortcut.keys}</kbd>
			</dt>
			<dd class="text-muted-foreground">{shortcut.description}</dd>
		{/each}
	</dl>
</Modal>
