<!-- Dashboard summary of Pangolin traffic over the last 7 days. -->
<script lang="ts">
	import Waypoints from '@lucide/svelte/icons/waypoints';
	import { Button } from '$lib/components/ui/button';
	import { fetchAnalytics, type Analytics } from '$lib/features/pangolin/client';
	import { formatNumber, formatPercent } from '$lib/format';
	import { api, toIpcError } from '$lib/ipc';
	import { workspace } from '$lib/workspace/workspace.svelte';

	let { visible }: { visible: boolean } = $props();

	let phase = $state<'loading' | 'unconfigured' | 'ready' | 'error'>('loading');
	let data = $state<Analytics | null>(null);
	let message = $state('');
	let loaded = false;

	async function load(): Promise<void> {
		try {
			const status = await api.pangolinStatus();
			if (!status.configured || !status.orgId) {
				phase = 'unconfigured';
				return;
			}
			data = await fetchAnalytics(24 * 7);
			phase = 'ready';
		} catch (raw) {
			const error = toIpcError(raw);
			phase = error.code === 'PANGOLIN_NOT_CONFIGURED' ? 'unconfigured' : 'error';
			message = error.message;
		}
	}

	$effect(() => {
		if (visible && !loaded) {
			loaded = true;
			void load();
		}
	});
</script>

<section class="rounded-xl border bg-card p-4">
	<div class="mb-2 flex items-center gap-2">
		<Waypoints class="size-4 text-primary" />
		<h3 class="text-sm font-semibold">Pangolin · last 7 days</h3>
		<Button variant="ghost" size="xs" class="ml-auto" onclick={() => workspace.openTab('pangolin')}
			>Open</Button
		>
	</div>
	{#if phase === 'loading'}
		<p class="text-xs text-muted-foreground">Loading…</p>
	{:else if phase === 'unconfigured'}
		<p class="text-xs text-muted-foreground">
			Connect Jarvis to your Pangolin API to see request statistics here.
		</p>
	{:else if phase === 'error'}
		<p class="selectable text-xs text-destructive">{message}</p>
	{:else if data}
		<div class="grid grid-cols-4 gap-3">
			<div>
				<p class="text-xs text-muted-foreground">Requests</p>
				<p class="text-lg font-semibold tabular">{formatNumber(data.total)}</p>
			</div>
			<div>
				<p class="text-xs text-muted-foreground">Allowed</p>
				<p class="text-lg font-semibold text-success tabular">{formatNumber(data.allowed)}</p>
			</div>
			<div>
				<p class="text-xs text-muted-foreground">Blocked</p>
				<p class="text-lg font-semibold text-destructive tabular">{formatNumber(data.blocked)}</p>
			</div>
			<div>
				<p class="text-xs text-muted-foreground">Block rate</p>
				<p class="text-lg font-semibold tabular">{formatPercent(data.blockRate, 1)}</p>
			</div>
		</div>
		{#if data.countries.length}
			<p class="mt-2 text-xs text-muted-foreground">
				Top countries:
				{#each data.countries.slice(0, 3) as c, i (c.code)}
					<span class="text-foreground tabular">{i ? ' · ' : ''}{c.code} {formatNumber(c.count)}</span>
				{/each}
			</p>
		{/if}
	{/if}
</section>
