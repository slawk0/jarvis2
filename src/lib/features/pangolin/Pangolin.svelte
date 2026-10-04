<!-- Pangolin Proxy: a client for the Pangolin Integration API (HTTP, not SSH). -->
<script lang="ts">
	import Waypoints from '@lucide/svelte/icons/waypoints';
	import Page from '$lib/components/Page.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { api, toIpcError, type IpcError, type PangolinStatus } from '$lib/ipc';
	import type { TabProps } from '$lib/workspace/registry';
	import PangolinAccess from './PangolinAccess.svelte';
	import PangolinClients from './PangolinClients.svelte';
	import PangolinDashboard from './PangolinDashboard.svelte';
	import PangolinLogs from './PangolinLogs.svelte';
	import PangolinResources from './PangolinResources.svelte';
	import PangolinSettings from './PangolinSettings.svelte';

	let { visible }: TabProps = $props();

	type View = 'dashboard' | 'logs' | 'resources' | 'access' | 'clients' | 'settings';

	let view = $state<View>('dashboard');
	let status = $state<PangolinStatus | null>(null);
	let error = $state<IpcError | null>(null);
	/** Bumped when the connection changes, so every sub-tab starts fresh. */
	let generation = $state(0);
	let dashboard = $state<{ refresh: () => unknown } | null>(null);
	let current = $state<{ refresh: () => unknown } | null>(null);

	const ready = $derived(!!status?.configured && !!status.orgId);

	async function loadStatus(): Promise<void> {
		try {
			status = await api.pangolinStatus();
			error = null;
		} catch (raw) {
			error = toIpcError(raw);
		}
	}

	let started = false;
	$effect(() => {
		if (!visible || started) return;
		started = true;
		void loadStatus();
	});

	async function changed(): Promise<void> {
		await loadStatus();
		generation++;
	}

	export function refresh(): void {
		if (view === 'settings' || !ready) void loadStatus();
		else void (view === 'dashboard' ? dashboard : current)?.refresh();
	}
</script>

{#if error && !status}
	<StateView kind="error" {error} onretry={loadStatus} />
{:else if !status}
	<StateView kind="loading" />
{:else}
	<Page scroll={false}>
		{#snippet header()}
			<SubTabs
				bind:value={view}
				items={[
					{ id: 'dashboard', label: 'Dashboard' },
					{ id: 'logs', label: 'Logs' },
					{ id: 'resources', label: 'Resources' },
					{ id: 'access', label: 'Access' },
					{ id: 'clients', label: 'Clients' },
					{ id: 'settings', label: 'Settings' }
				]}
			/>
		{/snippet}
		<div class="flex min-h-0 flex-1 flex-col pt-3">
			{#if view === 'settings'}
				<PangolinSettings {status} onchange={changed} />
			{:else if !ready}
				<StateView
					kind="empty"
					icon={Waypoints}
					title={status.configured ? 'Choose an organisation' : 'Pangolin is not connected yet'}
					message="Enter the API URL, an API key and the organisation in Settings."
				>
					<Button onclick={() => (view = 'settings')}>Open settings</Button>
				</StateView>
			{:else}
				{#key generation}
					<!-- The dashboard stays mounted so switching back is instant and keeps its filters. -->
					<div class={view === 'dashboard' ? 'flex min-h-0 flex-1 flex-col' : 'hidden'}>
						<PangolinDashboard bind:this={dashboard} visible={visible && view === 'dashboard'} />
					</div>
					{#if view === 'logs'}<PangolinLogs bind:this={current} {visible} />{/if}
					{#if view === 'resources'}<PangolinResources bind:this={current} {visible} />{/if}
					{#if view === 'access'}<PangolinAccess bind:this={current} {visible} />{/if}
					{#if view === 'clients'}<PangolinClients bind:this={current} {visible} />{/if}
				{/key}
			{/if}
		</div>
	</Page>
{/if}
