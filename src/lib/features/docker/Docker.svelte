<script lang="ts">
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { apiQuiet } from '$lib/ipc';
	import { app } from '$lib/services/app.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { poll, resource } from '$lib/state/resource.svelte';
	import { debounce } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';
	import Compose from './Compose.svelte';
	import Containers from './Containers.svelte';
	import Images from './Images.svelte';
	import Networks from './Networks.svelte';
	import Stats from './Stats.svelte';
	import Volumes from './Volumes.svelte';

	let { visible }: TabProps = $props();

	type View = 'containers' | 'images' | 'networks' | 'volumes' | 'compose' | 'stats';
	let view = $state<View>('containers');
	/** Sub-tabs opened so far stay mounted, like panes keep their tabs. */
	let mounted = $state<View[]>(['containers']);
	const overview = resource((io) => io.dockerOverview());

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void overview.refresh();
		}
	});
	poll(
		() => visible,
		15_000,
		() => overview.load(true)
	);

	$effect(() => {
		if (!mounted.includes(view)) mounted = [...mounted, view];
	});

	const sections = {
		containers: Containers,
		images: Images,
		networks: Networks,
		volumes: Volumes,
		compose: Compose,
		stats: Stats
	};
	const instances: Partial<
		Record<View, { refresh?: () => unknown; sync?: () => unknown; reset?: () => void }>
	> = {};
	/** Sub-tabs whose data changed while they were hidden. */
	const stale = new Set<View>();

	/** Docker changed, possibly outside Jarvis: reload what is on screen, flag the rest. */
	const changed = debounce(() => {
		void overview.load(true);
		for (const id of mounted) stale.add(id);
		syncCurrent();
	}, 400);

	function syncCurrent(): void {
		if (!visible || !stale.delete(view)) return;
		void instances[view]?.sync?.();
	}

	$effect(() => {
		void view;
		syncCurrent();
	});

	// Follow `docker events` while the tab is on screen, so containers started, stopped or
	// rebuilt from a terminal show up at once. Without the stream (sudo not unlocked yet,
	// old Docker) the sections still poll.
	$effect(() => {
		if (!visible || !app.online) return;
		let job: Job | null = null;
		let off: (() => void) | undefined;
		let stopped = false;
		apiQuiet.dockerEventsFollow().then(
			(id) => {
				job = jobs.get(id);
				if (stopped) return void job.stop();
				off = job.onOutput((chunk) => {
					// Healthchecks emit exec events every few seconds; they change nothing we show.
					if (chunk.split('\n').some((line) => line.trim() && !line.startsWith('container exec_'))) changed();
				});
			},
			() => {}
		);
		return () => {
			stopped = true;
			off?.();
			void job?.stop();
			changed.cancel();
		};
	});

	// Whatever happened while the tab was hidden was not seen: catch up when it is shown again.
	let wasHidden = false;
	$effect(() => {
		if (!visible) wasHidden = started;
		else if (wasHidden) {
			wasHidden = false;
			changed();
		}
	});
	const o = $derived(overview.data);

	export function refresh(): void {
		void overview.refresh();
		void instances[view]?.refresh?.();
	}

	export function onReselect(): void {
		instances[view]?.reset?.();
	}
</script>

<div class="flex h-full min-h-0 flex-col">
	<div class="flex shrink-0 items-end gap-4 px-4 pt-2">
		<SubTabs
			class="flex-1"
			bind:value={view}
			items={[
				{ id: 'containers', label: 'Containers', count: o ? o.running + o.stopped : null },
				{ id: 'images', label: 'Images', count: o?.images },
				{ id: 'networks', label: 'Networks', count: o?.networks },
				{ id: 'volumes', label: 'Volumes', count: o?.volumes },
				{ id: 'compose', label: 'Compose' },
				{ id: 'stats', label: 'Stats' }
			]}
		/>
		{#if o}
			<p class="pb-1.5 text-xs text-muted-foreground tabular">
				<span class="text-success">{o.running} running</span> · {o.stopped} stopped · Docker {o.version}
			</p>
		{/if}
	</div>
	<div class="relative min-h-0 flex-1">
		{#each mounted as id (id)}
			{@const Section = sections[id]}
			<div class="absolute inset-0" class:hidden={view !== id}>
				<Section
					bind:this={instances[id]}
					visible={visible && view === id}
					onchanged={() => overview.load(true)}
				/>
			</div>
		{/each}
	</div>
</div>
