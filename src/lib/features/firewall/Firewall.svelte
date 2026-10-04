<script lang="ts">
	import AsyncView from '$lib/components/AsyncView.svelte';
	import DependencyGuard from '$lib/components/DependencyGuard.svelte';
	import Page from '$lib/components/Page.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { autoLoad, resource } from '$lib/state/resource.svelte';
	import type { TabProps } from '$lib/workspace/registry';
	import Iptables from './Iptables.svelte';
	import Ufw from './Ufw.svelte';

	let { visible }: TabProps = $props();

	const backends = resource((io) => io.firewallDetect());
	let mode = $state<'ufw' | 'iptables'>('ufw');
	let chosen = false;
	let ufw = $state<Ufw | null>(null);
	let iptables = $state<Iptables | null>(null);

	autoLoad(backends, () => visible);

	// Prefer UFW when it is installed; otherwise start on iptables.
	$effect(() => {
		const b = backends.data;
		if (b && !chosen) {
			chosen = true;
			mode = b.ufw || !b.iptables ? 'ufw' : 'iptables';
		}
	});

	export function refresh(): void {
		void backends.refresh();
		void (mode === 'ufw' ? ufw : iptables)?.refresh();
	}
</script>

<Page scroll={false}>
	{#snippet header()}
		<SubTabs
			bind:value={mode}
			items={[
				{ id: 'ufw', label: 'UFW' },
				{ id: 'iptables', label: 'iptables' }
			]}
		/>
	{/snippet}
	<AsyncView resource={backends}>
		{#snippet children(b)}
			{#if b.firewalld || (b.nftables && !b.ufw && !b.iptables)}
				<p class="mb-3 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-xs">
					{b.firewalld ? 'firewalld is active on this server.' : 'This server uses nftables.'}
					Jarvis does not manage {b.firewalld ? 'firewalld' : 'nftables'} rules; changes made here with UFW or iptables
					may conflict with it.
				</p>
			{/if}
			{#if !b.ufw && !b.iptables && mode === 'ufw'}
				<StateView
					kind="empty"
					title="No firewall tool found"
					message="Install UFW (simple) or iptables to manage the firewall from here."
					compact
				/>
			{/if}
			{#if mode === 'ufw'}
				<DependencyGuard tools={['ufw']} inline active={visible}>
					<Ufw bind:this={ufw} {visible} sshPort={b.sshPort} />
				</DependencyGuard>
			{:else}
				<DependencyGuard tools={['iptables']} inline active={visible}>
					<Iptables bind:this={iptables} {visible} />
				</DependencyGuard>
			{/if}
		{/snippet}
	</AsyncView>
</Page>
