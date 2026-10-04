<!--
	Mounts one tab inside a pane and keeps it alive while hidden. Loads the
	feature lazily and wraps it in the dependency guard when it needs tools.
-->
<script lang="ts">
	import DependencyGuard from '$lib/components/DependencyGuard.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { toIpcError } from '$lib/ipc';
	import { app } from '$lib/services/app.svelte';
	import type { TabComponent, TabDef, TabExports } from './registry';
	import { workspace } from './workspace.svelte';

	interface Props {
		paneId: string;
		def: TabDef;
		visible: boolean;
	}

	let { paneId, def, visible }: Props = $props();

	let Feature = $state<TabComponent | null>(null);
	let loadError = $state<unknown>(null);
	let instance = $state<TabExports | null>(null);

	async function load(): Promise<void> {
		loadError = null;
		try {
			Feature = (await def.load()).default;
		} catch (error) {
			loadError = error;
		}
	}

	void load();

	$effect(() => {
		workspace.registerInstance(paneId, def.id, instance);
		return () => workspace.registerInstance(paneId, def.id, null);
	});
</script>

<div
	class="absolute inset-0"
	class:hidden={!visible}
	role="tabpanel"
	aria-label={def.label}
	aria-hidden={!visible}
>
	{#if loadError}
		<StateView kind="error" error={toIpcError(loadError)} onretry={load} />
	{:else if Feature && app.profile}
		<DependencyGuard tools={def.requires} active={visible}>
			<Feature bind:this={instance} {visible} profile={app.profile} />
		</DependencyGuard>
	{:else}
		<StateView kind="loading" />
	{/if}
</div>
