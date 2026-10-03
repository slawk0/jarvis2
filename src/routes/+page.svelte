<script lang="ts">
	import { onMount } from 'svelte';
	import ConfirmHost from '$lib/components/ConfirmHost.svelte';
	import JobsPanel from '$lib/components/JobsPanel.svelte';
	import SudoDialog from '$lib/components/SudoDialog.svelte';
	import Toaster from '$lib/components/Toaster.svelte';
	import { api, toIpcError } from '$lib/ipc';
	import HostKeyDialog from '$lib/screens/HostKeyDialog.svelte';
	import Login from '$lib/screens/Login.svelte';
	import Settings from '$lib/screens/Settings.svelte';
	import UpdateDialog from '$lib/screens/UpdateDialog.svelte';
	import { app } from '$lib/services/app.svelte';
	import { jobs } from '$lib/services/jobs.svelte';
	import { settings } from '$lib/services/settings.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import WorkspaceView from '$lib/workspace/WorkspaceView.svelte';

	let ready = $state(false);

	onMount(() => {
		void (async () => {
			try {
				await Promise.all([settings.load(), jobs.init()]);
				await app.init();
				for (const notice of await api.startupNotices()) toast.error(toIpcError(notice));
			} catch (error) {
				toast.error(error, 'Jarvis could not start properly');
			} finally {
				ready = true;
			}
		})();

		// A desktop app has no use for the browser context menu.
		const blockMenu = (e: MouseEvent) => {
			const target = e.target as HTMLElement | null;
			if (!target?.closest('input, textarea, [contenteditable="true"], [data-native-menu]')) {
				e.preventDefault();
			}
		};
		window.addEventListener('contextmenu', blockMenu);
		return () => window.removeEventListener('contextmenu', blockMenu);
	});
</script>

<svelte:head>
	<title>Jarvis Server Manager</title>
</svelte:head>

{#if ready}
	{#if app.session}
		{#key app.sessionKey}
			<WorkspaceView />
		{/key}
	{:else}
		<Login />
	{/if}
{/if}

<HostKeyDialog />
<Settings />
<UpdateDialog />
<JobsPanel />
<SudoDialog />
<ConfirmHost />
<Toaster />
