<script lang="ts">
	import ArrowUpCircle from '@lucide/svelte/icons/circle-arrow-up';
	import Power from '@lucide/svelte/icons/power';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import AsyncView from '$lib/components/AsyncView.svelte';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import JobDialog from '$lib/components/JobDialog.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { Button } from '$lib/components/ui/button';
	import { api, toIpcError, type PackageHit, type PendingUpdate } from '$lib/ipc';
	import { app } from '$lib/services/app.svelte';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';

	let { visible }: TabProps = $props();

	const status = resource((io) => io.maintenanceStatus());
	let job = $state<Job | null>(null);
	let query = $state('');
	let hits = $state<PackageHit[] | null>(null);
	let searching = $state(false);
	let filter = $state('');

	autoLoad(status, () => visible);

	const columns: Column<PendingUpdate>[] = [
		{ key: 'name', label: 'Package', value: (u) => u.name, class: 'max-w-0 w-full' },
		{ key: 'current', label: 'Installed', value: (u) => u.current, mono: true, class: 'w-64 max-w-64' },
		{ key: 'candidate', label: 'Available', value: (u) => u.candidate, mono: true, class: 'w-64 max-w-64' }
	];
	const NAMES = { apt: 'APT', dnf: 'DNF', yum: 'YUM', apk: 'apk', pacman: 'pacman', zypper: 'zypper' };

	async function runJob(reason: string, start: () => Promise<string>): Promise<void> {
		try {
			const id = await sudo.describe(reason, start);
			job = jobs.get(id);
			await job.wait();
			await status.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	async function upgrade(): Promise<void> {
		const updates = status.data?.updates ?? [];
		const ok = await confirm({
			title: `Upgrade ${updates.length} package${updates.length === 1 ? '' : 's'}?`,
			message: 'Services may be restarted while packages are upgraded.',
			detail: updates.map((u) => `${u.name}  ${u.current ? `${u.current} → ` : ''}${u.candidate}`).join('\n'),
			confirmLabel: 'Upgrade all'
		});
		if (ok) await runJob('Upgrade all packages', () => api.maintenanceUpgrade());
	}

	async function reboot(): Promise<void> {
		const ok = await confirm({
			title: `Reboot ${app.profile?.label ?? 'the server'}?`,
			message:
				'Everything running on the server is interrupted. Jarvis reconnects automatically when it is back.',
			confirmLabel: 'Reboot now',
			destructive: true
		});
		if (!ok) return;
		try {
			await sudo.describe('Reboot the server', () => api.maintenanceReboot());
			app.rebooting = true;
		} catch (error) {
			toast.error(error, 'Could not reboot');
		}
	}

	async function search(): Promise<void> {
		if (query.trim().length < 2) return;
		searching = true;
		try {
			hits = await api.packageSearch(query.trim());
		} catch (error) {
			toast.error(toIpcError(error));
		} finally {
			searching = false;
		}
	}

	async function change(name: string, install: boolean): Promise<void> {
		if (!install) {
			const ok = await confirm({
				title: `Remove ${name}?`,
				message: 'Packages that depend on it may be removed as well.',
				confirmLabel: 'Remove',
				destructive: true
			});
			if (!ok) return;
		}
		await runJob(`${install ? 'Install' : 'Remove'} ${name}`, () => api.packageChange(name, install));
	}

	export const refresh = () => status.refresh();
</script>

<Page>
	{#snippet toolbar()}
		<span class="flex-1"></span>
		<RefreshControl onrefresh={status.refresh} loading={status.loading} label="Re-read package status" />
	{/snippet}
	<AsyncView resource={status} loadingTitle="Checking for updates…">
		{#snippet children(s)}
			{#if !s.packageManager}
				<StateView
					kind="empty"
					title="No supported package manager"
					message="Jarvis manages packages with apt, dnf, yum, apk, pacman and zypper. None of them was found on this server."
					compact
				/>
			{:else}
				<div class="grid grid-cols-1 gap-3 lg:grid-cols-3">
					<section class="flex flex-col gap-2 rounded-xl border bg-card p-4">
						<div class="flex items-center gap-2">
							<ArrowUpCircle class={cn('size-5', s.updates.length ? 'text-warning' : 'text-success')} />
							<h3 class="text-sm font-semibold">Updates</h3>
							<span class="ml-auto text-xs text-muted-foreground">{NAMES[s.packageManager]}</span>
						</div>
						<p class="text-2xl font-semibold tabular">{s.updates.length}</p>
						<p class="text-xs text-muted-foreground">
							{s.updates.length ? 'packages can be upgraded' : 'Everything is up to date'}
						</p>
						<div class="mt-auto flex gap-2 pt-1">
							<Button
								variant="outline"
								size="sm"
								onclick={() => runJob('Refresh the package index', () => api.maintenanceRefresh())}
								>Refresh index</Button
							>
							<Button size="sm" disabled={s.updates.length === 0} onclick={upgrade}>Upgrade all</Button>
						</div>
					</section>

					<section class="flex flex-col gap-2 rounded-xl border bg-card p-4">
						<div class="flex items-center gap-2">
							<Power class={cn('size-5', s.rebootRequired ? 'text-warning' : 'text-muted-foreground')} />
							<h3 class="text-sm font-semibold">Reboot</h3>
						</div>
						<p class="text-sm font-medium">{s.rebootRequired ? 'Reboot required' : 'No reboot needed'}</p>
						{#if s.rebootReasons.length}
							<p class="selectable line-clamp-3 text-xs text-muted-foreground">
								Requested by: {s.rebootReasons.join(', ')}
							</p>
						{/if}
						<div class="mt-auto pt-1">
							<Button variant={s.rebootRequired ? 'default' : 'outline'} size="sm" onclick={reboot}
								>Reboot server…</Button
							>
						</div>
					</section>

					<section class="flex flex-col gap-2 rounded-xl border bg-card p-4">
						<div class="flex items-center gap-2">
							<ShieldCheck
								class={cn('size-5', s.autoUpdates === 'enabled' ? 'text-success' : 'text-muted-foreground')}
							/>
							<h3 class="text-sm font-semibold">Automatic updates</h3>
						</div>
						<p class="text-sm font-medium">
							{#if s.autoUpdates === 'enabled'}Enabled
							{:else if s.autoUpdates === 'disabled'}Disabled
							{:else if s.autoUpdates === 'notInstalled'}Not installed
							{:else}Not managed for {NAMES[s.packageManager]}{/if}
						</p>
						<p class="text-xs text-muted-foreground">
							{s.packageManager === 'apt'
								? 'unattended-upgrades installs security updates on its own.'
								: s.packageManager === 'dnf' || s.packageManager === 'yum'
									? 'dnf-automatic installs updates on a timer.'
									: 'Use your distribution’s tools for unattended updates.'}
						</p>
						{#if s.autoUpdates !== 'unsupported'}
							<div class="mt-auto pt-1">
								{#if s.autoUpdates === 'enabled'}
									<Button
										variant="outline"
										size="sm"
										onclick={() =>
											runJob('Disable automatic updates', () => api.maintenanceAutoUpdates(false))}
										>Disable</Button
									>
								{:else}
									<Button
										variant="outline"
										size="sm"
										onclick={() => runJob('Enable automatic updates', () => api.maintenanceAutoUpdates(true))}
									>
										{s.autoUpdates === 'notInstalled' ? 'Install and enable' : 'Enable'}
									</Button>
								{/if}
							</div>
						{/if}
					</section>
				</div>

				<div class="mt-5 mb-2 flex items-center gap-3">
					<h3 class="text-sm font-semibold">Pending updates</h3>
					<SearchInput bind:value={filter} placeholder="Filter…" class="w-56" />
				</div>
				<DataTable
					rows={s.updates}
					{columns}
					rowKey={(u) => u.name}
					search={filter}
					empty="No pending updates"
					emptyHint="Refresh the index to check again."
					class="max-h-96"
				/>

				<h3 class="mt-6 mb-2 text-sm font-semibold">Install or remove a package</h3>
				<form
					class="flex items-center gap-2"
					onsubmit={(e) => {
						e.preventDefault();
						void search();
					}}
				>
					<SearchInput bind:value={query} placeholder="Search packages…" class="w-72" />
					<Button type="submit" variant="outline" size="sm" disabled={searching || query.trim().length < 2}
						>{searching ? 'Searching…' : 'Search'}</Button
					>
				</form>
				{#if hits}
					<ul class="mt-2 max-h-80 divide-y overflow-y-auto rounded-lg border bg-card">
						{#each hits as hit (hit.name)}
							<li class="flex items-center gap-3 px-3 py-1.5">
								<div class="selectable min-w-0 flex-1">
									<p class="truncate font-mono text-xs font-medium">{hit.name}</p>
									<p class="truncate text-xs text-muted-foreground">{hit.description}</p>
								</div>
								<Button variant="outline" size="xs" onclick={() => change(hit.name, true)}>Install</Button>
								<Button variant="ghost" size="xs" onclick={() => change(hit.name, false)}>Remove</Button>
							</li>
						{:else}
							<li class="p-3 text-sm text-muted-foreground">No packages found.</li>
						{/each}
					</ul>
				{/if}
			{/if}
		{/snippet}
	</AsyncView>
</Page>

<JobDialog bind:job />
