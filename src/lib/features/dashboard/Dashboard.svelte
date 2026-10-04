<script lang="ts">
	import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
	import ArrowDown from '@lucide/svelte/icons/arrow-down';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import BellRing from '@lucide/svelte/icons/bell-ring';
	import AsyncView from '$lib/components/AsyncView.svelte';
	import LineChart from '$lib/components/LineChart.svelte';
	import Page from '$lib/components/Page.svelte';
	import { Button } from '$lib/components/ui/button';
	import { formatBytes, formatDuration, formatSpeed } from '$lib/format';
	import type { BasicStats } from '$lib/ipc';
	import { loadDoc, type AlertThresholds } from '$lib/services/profile-data';
	import { poll, resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';
	import { workspace } from '$lib/workspace/workspace.svelte';
	import Gauge from './Gauge.svelte';
	import PangolinCard from './PangolinCard.svelte';

	let { visible, profile }: TabProps = $props();

	const HISTORY = 60;
	const ALERT_COOLDOWN = 5 * 60_000;

	const basic = resource((io) => io.statsBasic());
	const extended = resource((io) => io.statsExtended());

	let previous: { stats: BasicStats; at: number } | null = null;
	let cpu = $state<number | null>(null);
	let rx = $state<number | null>(null);
	let tx = $state<number | null>(null);
	let cpuHistory = $state<number[]>([]);
	let ramHistory = $state<number[]>([]);
	let thresholds = $state<AlertThresholds>({ enabled: false, cpu: 95, ram: 90, disk: 85 });
	const lastAlert: Record<string, number> = {};

	const stats = $derived(basic.data);
	const ram = $derived(stats ? ((stats.memTotal - stats.memAvailable) / stats.memTotal) * 100 : null);
	const disk = $derived(stats && stats.rootTotal ? (stats.rootUsed / stats.rootTotal) * 100 : null);

	async function sample(quiet: boolean): Promise<void> {
		await basic.load(quiet);
		const now = Date.now();
		const current = basic.data;
		if (!current) return;
		if (previous) {
			const total = current.cpuTotal - previous.stats.cpuTotal;
			const idle = current.cpuIdle - previous.stats.cpuIdle;
			const seconds = (now - previous.at) / 1000;
			if (total > 0) cpu = Math.max(0, Math.min(100, (1 - idle / total) * 100));
			if (seconds > 0 && current.netInterface === previous.stats.netInterface) {
				rx = Math.max(0, (current.netRxBytes - previous.stats.netRxBytes) / seconds);
				tx = Math.max(0, (current.netTxBytes - previous.stats.netTxBytes) / seconds);
			}
		}
		previous = { stats: current, at: now };
		if (cpu !== null) cpuHistory = [...cpuHistory, cpu].slice(-HISTORY);
		const usedRam = ((current.memTotal - current.memAvailable) / current.memTotal) * 100;
		ramHistory = [...ramHistory, usedRam].slice(-HISTORY);
		void checkAlerts(usedRam, current.rootTotal ? (current.rootUsed / current.rootTotal) * 100 : null);
	}

	async function checkAlerts(ramNow: number, diskNow: number | null): Promise<void> {
		if (!thresholds.enabled) return;
		const metrics: [string, number | null, number][] = [
			['CPU', cpu, thresholds.cpu],
			['RAM', ramNow, thresholds.ram],
			['Disk', diskNow, thresholds.disk]
		];
		for (const [name, value, limit] of metrics) {
			if (value === null || value < limit) continue;
			if (Date.now() - (lastAlert[name] ?? 0) < ALERT_COOLDOWN) continue;
			lastAlert[name] = Date.now();
			let granted = await isPermissionGranted();
			if (!granted) granted = (await requestPermission()) === 'granted';
			if (granted) {
				sendNotification({
					title: `${profile.label}: ${name} at ${Math.round(value)}%`,
					body: `${name} usage is above the ${limit}% threshold.`
				});
			}
		}
	}

	async function loadThresholds(): Promise<void> {
		try {
			thresholds = await loadDoc('alertThresholds', thresholds);
		} catch {
			// Alerts stay off if the settings cannot be read.
		}
	}

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void sample(false);
			void extended.refresh();
			void loadThresholds();
		}
	});
	// Re-read thresholds when the settings dialog closes.
	$effect(() => {
		if (!workspace.settingsOpen && started) void loadThresholds();
	});
	poll(
		() => visible,
		2000,
		() => sample(true)
	);
	poll(
		() => visible,
		10_000,
		() => extended.load(true)
	);

	export function refresh(): void {
		void sample(false);
		void extended.refresh();
	}
</script>

<Page>
	<AsyncView resource={basic}>
		{#snippet children(s)}
			<div class="grid grid-cols-1 gap-3 xl:grid-cols-3">
				<button
					type="button"
					class="rounded-xl border bg-card p-4 text-left transition-colors hover:border-primary/40"
					onclick={() => workspace.openTab('processes')}
				>
					<Gauge
						label="CPU"
						percent={cpu}
						detail="{s.cpuCount} core{s.cpuCount === 1 ? '' : 's'} · load {s.load[0].toFixed(2)}"
					/>
				</button>
				<button
					type="button"
					class="rounded-xl border bg-card p-4 text-left transition-colors hover:border-primary/40"
					onclick={() => workspace.openTab('processes')}
				>
					<Gauge
						label="Memory"
						percent={ram}
						detail="{formatBytes(s.memTotal - s.memAvailable)} of {formatBytes(s.memTotal)}"
					/>
				</button>
				<button
					type="button"
					class="rounded-xl border bg-card p-4 text-left transition-colors hover:border-primary/40"
					onclick={() => workspace.openTab('disks')}
				>
					<Gauge
						label="Disk /"
						percent={disk}
						warn={65}
						danger={85}
						detail="{formatBytes(s.rootUsed)} of {formatBytes(s.rootTotal)}"
					/>
				</button>
			</div>

			<div class="mt-3 grid grid-cols-1 gap-3 xl:grid-cols-3">
				<section class="rounded-xl border bg-card p-4 xl:col-span-2">
					<h3 class="mb-2 text-sm font-semibold">CPU and memory</h3>
					<LineChart
						max={100}
						slots={HISTORY}
						format={(v) => `${Math.round(v)}%`}
						series={[
							{ label: 'CPU', color: 'var(--primary)', values: cpuHistory },
							{ label: 'Memory', color: 'var(--info)', values: ramHistory }
						]}
					/>
				</section>

				<section class="flex flex-col gap-3 rounded-xl border bg-card p-4">
					<h3 class="text-sm font-semibold">
						Network <span class="font-mono text-xs font-normal text-muted-foreground"
							>{s.netInterface || 'no interface'}</span
						>
					</h3>
					<div class="grid grid-cols-2 gap-3">
						<div>
							<p class="flex items-center gap-1 text-xs text-muted-foreground">
								<ArrowDown class="size-3.5 text-success" /> Download
							</p>
							<p class="text-lg font-semibold tabular">{formatSpeed(rx)}</p>
						</div>
						<div>
							<p class="flex items-center gap-1 text-xs text-muted-foreground">
								<ArrowUp class="size-3.5 text-info" /> Upload
							</p>
							<p class="text-lg font-semibold tabular">{formatSpeed(tx)}</p>
						</div>
					</div>
					<dl class="mt-auto grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 border-t pt-3 text-xs">
						<dt class="text-muted-foreground">Uptime</dt>
						<dd class="tabular">{formatDuration(s.uptimeSecs)}</dd>
						<dt class="text-muted-foreground">Load average</dt>
						<dd class="tabular">{s.load.map((l) => l.toFixed(2)).join(' · ')}</dd>
						<dt class="text-muted-foreground">Swap</dt>
						<dd class="tabular">
							{s.swapTotal
								? `${formatBytes(s.swapTotal - s.swapFree)} of ${formatBytes(s.swapTotal)}`
								: 'none'}
						</dd>
						{#if extended.data}
							<dt class="text-muted-foreground">System</dt>
							<dd class="selectable truncate">{extended.data.system.os}</dd>
							<dt class="text-muted-foreground">Hostname</dt>
							<dd class="selectable truncate">{extended.data.system.hostname}</dd>
							<dt class="text-muted-foreground">Kernel</dt>
							<dd class="selectable truncate">{extended.data.system.kernel}</dd>
						{/if}
					</dl>
				</section>
			</div>

			<div class="mt-3 grid grid-cols-1 gap-3 xl:grid-cols-2">
				<section class="rounded-xl border bg-card p-4">
					<h3 class="mb-2 text-sm font-semibold">Disk partitions</h3>
					{#if extended.data}
						<table class="w-full text-sm">
							<thead class="text-xs text-muted-foreground">
								<tr
									><th class="pb-1 text-left font-medium">Mount</th><th class="pb-1 text-right font-medium"
										>Used</th
									><th class="w-32 pb-1 pl-3 text-left font-medium">Usage</th><th
										class="pb-1 text-right font-medium">Inodes</th
									></tr
								>
							</thead>
							<tbody>
								{#each extended.data.partitions as p (p.mount)}
									<tr class="border-t">
										<td class="selectable max-w-40 truncate py-1.5 font-mono text-xs">{p.mount}</td>
										<td class="py-1.5 text-right text-xs tabular"
											>{formatBytes(p.used)} / {formatBytes(p.total)}</td
										>
										<td class="py-1.5 pl-3">
											<div class="flex items-center gap-2">
												<div class="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
													<div
														class={cn(
															'h-full rounded-full',
															p.usePercent > 85
																? 'bg-destructive'
																: p.usePercent > 65
																	? 'bg-warning'
																	: 'bg-primary'
														)}
														style="width: {p.usePercent}%"
													></div>
												</div>
												<span class="w-9 text-right text-xs tabular">{Math.round(p.usePercent)}%</span>
											</div>
										</td>
										<td
											class={cn(
												'py-1.5 text-right text-xs tabular',
												(p.inodePercent ?? 0) >= 85 && 'font-medium text-warning'
											)}
										>
											{p.inodePercent == null ? '—' : `${Math.round(p.inodePercent)}%`}
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{:else}
						<p class="text-xs text-muted-foreground">Loading…</p>
					{/if}
				</section>

				<section class="rounded-xl border bg-card p-4">
					<div class="mb-2 flex items-center">
						<h3 class="text-sm font-semibold">Top processes by memory</h3>
						<Button variant="ghost" size="xs" class="ml-auto" onclick={() => workspace.openTab('processes')}
							>All processes</Button
						>
					</div>
					{#if extended.data}
						<table class="w-full text-sm">
							<thead class="text-xs text-muted-foreground">
								<tr
									><th class="pb-1 text-left font-medium">PID</th><th class="pb-1 text-left font-medium"
										>User</th
									><th class="pb-1 text-right font-medium">CPU</th><th class="pb-1 text-right font-medium"
										>MEM</th
									><th class="pb-1 pl-3 text-left font-medium">Command</th></tr
								>
							</thead>
							<tbody>
								{#each extended.data.topProcesses as p (p.pid)}
									<tr class="border-t">
										<td class="py-1.5 text-xs tabular">{p.pid}</td>
										<td class="max-w-24 truncate py-1.5 text-xs">{p.user}</td>
										<td class="py-1.5 text-right text-xs tabular">{p.cpu.toFixed(1)}%</td>
										<td class="py-1.5 text-right text-xs tabular">{p.mem.toFixed(1)}%</td>
										<td class="selectable max-w-0 truncate py-1.5 pl-3 font-mono text-xs" style="width: 55%"
											>{p.command}</td
										>
									</tr>
								{/each}
							</tbody>
						</table>
					{:else}
						<p class="text-xs text-muted-foreground">Loading…</p>
					{/if}
				</section>
			</div>

			<div class="mt-3 grid grid-cols-1 gap-3 xl:grid-cols-2">
				<PangolinCard {visible} />
				<section class="flex items-center gap-3 rounded-xl border bg-card p-4">
					<BellRing
						class={cn('size-5 shrink-0', thresholds.enabled ? 'text-primary' : 'text-muted-foreground')}
					/>
					<div class="min-w-0 flex-1">
						<h3 class="text-sm font-semibold">Desktop alerts</h3>
						<p class="text-xs text-muted-foreground">
							{#if thresholds.enabled}
								Notifying above CPU {thresholds.cpu}% · RAM {thresholds.ram}% · Disk {thresholds.disk}%
							{:else}
								Off for this server.
							{/if}
						</p>
					</div>
					<Button variant="outline" size="sm" onclick={() => workspace.openSettings('alerts')}
						>Configure</Button
					>
				</section>
			</div>
		{/snippet}
	</AsyncView>
</Page>
