<script lang="ts">
	import DependencyGuard from '$lib/components/DependencyGuard.svelte';
	import Field from '$lib/components/Field.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import TargetProfiles, { execTarget } from '$lib/components/TargetProfiles.svelte';
	import { Input } from '$lib/components/ui/input';
	import type { NginxTarget as BackendTarget } from '$lib/ipc';
	import { loadDoc, saveDoc, type NginxTarget } from '$lib/services/profile-data';
	import { resource } from '$lib/state/resource.svelte';
	import type { TabProps } from '$lib/workspace/registry';
	import Certificates from './Certificates.svelte';
	import ConfigFiles from './ConfigFiles.svelte';
	import Control from './Control.svelte';
	import ProxyHosts from './ProxyHosts.svelte';

	let { visible }: TabProps = $props();

	type View = 'hosts' | 'certificates' | 'files' | 'control';
	let view = $state<View>('hosts');
	let selectedId = $state('');
	const targets = resource(() => loadDoc('nginxTargets', []));
	const instances: Partial<Record<View, { refresh?: () => unknown; reset?: () => void }>> = {};

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void targets.refresh();
		}
	});
	$effect(() => {
		const list = targets.data;
		if (list?.length && !list.some((t) => t.id === selectedId)) selectedId = list[0].id;
	});

	const selected = $derived(targets.data?.find((t) => t.id === selectedId));
	const target = $derived<BackendTarget | null>(
		selected ? { target: execTarget(selected), configRoot: selected.configRoot } : null
	);

	async function save(next: NginxTarget[]): Promise<void> {
		await saveDoc('nginxTargets', next);
		targets.set(next);
	}

	export function refresh(): void {
		void instances[view]?.refresh?.();
	}
	export function onReselect(): void {
		instances[view]?.reset?.();
	}
</script>

{#snippet profiles()}
	<TargetProfiles
		noun="Nginx target"
		description="Tell Jarvis where nginx runs: on the server itself, or inside a Docker container."
		profiles={targets.data ?? []}
		bind:selectedId
		blank={() => ({ id: '', name: 'Nginx', kind: 'host', container: '', configRoot: '/etc/nginx' })}
		{save}
		validate={(p) =>
			p.configRoot.startsWith('/') ? null : 'The configuration directory must be an absolute path.'}
	>
		{#snippet fields(p)}
			<Field label="Configuration directory"
				><Input bind:value={p.configRoot} class="font-mono text-xs" spellcheck="false" /></Field
			>
		{/snippet}
	</TargetProfiles>
{/snippet}

{#if targets.error && !targets.loaded}
	<StateView kind="error" error={targets.error} onretry={targets.refresh} />
{:else if !targets.loaded}
	<StateView kind="loading" />
{:else if !target || !selected}
	{@render profiles()}
{:else}
	<div class="flex h-full min-h-0 flex-col">
		<div class="flex shrink-0 items-end gap-3 px-4 pt-2">
			<SubTabs
				class="flex-1"
				bind:value={view}
				items={[
					{ id: 'hosts', label: 'Proxy hosts' },
					{ id: 'certificates', label: 'SSL certificates' },
					{ id: 'files', label: 'Config files' },
					{ id: 'control', label: 'Control' }
				]}
			/>
			<div class="pb-1.5">{@render profiles()}</div>
		</div>
		<div class="min-h-0 flex-1">
			{#key selectedId}
				<!-- Tools can only be installed automatically on the host. -->
				<DependencyGuard tools={selected.kind === 'host' ? ['nginx'] : []} active={visible}>
					{#if view === 'hosts'}
						<ProxyHosts bind:this={instances.hosts} {visible} {target} />
					{:else if view === 'certificates'}
						<Certificates
							bind:this={instances.certificates}
							{visible}
							{target}
							host={selected.kind === 'host'}
						/>
					{:else if view === 'files'}
						<ConfigFiles bind:this={instances.files} {visible} {target} />
					{:else}
						<Control bind:this={instances.control} {visible} {target} />
					{/if}
				</DependencyGuard>
			{/key}
		</div>
	</div>
{/if}
