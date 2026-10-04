<script lang="ts">
	import Copy from '@lucide/svelte/icons/copy';
	import HardDrive from '@lucide/svelte/icons/hard-drive';
	import DependencyGuard from '$lib/components/DependencyGuard.svelte';
	import Field from '$lib/components/Field.svelte';
	import JobDialog from '$lib/components/JobDialog.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import { formatBytes } from '$lib/format';
	import { api, type BlockDevice, type NewFs, type TableKind } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, resource } from '$lib/state/resource.svelte';
	import { cn, copyText } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';

	let { visible }: TabProps = $props();

	const usage = resource((io) => io.disksUsage());
	const devices = resource((io) => io.disksDevices());
	let showSystem = $state(false);
	let showLoop = $state(false);
	let job = $state<Job | null>(null);
	let mounting = $state<BlockDevice | null>(null);
	let mountPoint = $state('/mnt/');
	let createDir = $state(true);
	let partitioning = $state<BlockDevice | null>(null);
	let partForm = $state({ mode: 'free' as 'free' | 'gpt' | 'mbr', filesystem: 'ext4' as NewFs, label: '' });

	autoLoad(usage, () => visible, 30_000);
	autoLoad(devices, () => visible);

	const filesystems = $derived(
		(usage.data ?? []).filter((f) => (showSystem || !f.system) && (showLoop || !f.isLoop))
	);
	const all = $derived(devices.data ?? []);
	const disks = $derived(all.filter((d) => d.depth === 0 && (showLoop || d.kind !== 'loop')));
	const childrenOf = (name: string): BlockDevice[] =>
		all.filter((d) => d.parent === name).flatMap((d) => [d, ...childrenOf(d.name)]);
	const partitionsOf = (name: string) => all.filter((d) => d.parent === name);
	const unallocated = (disk: BlockDevice) =>
		Math.max(0, disk.size - partitionsOf(disk.name).reduce((sum, p) => sum + p.size, 0));

	const COLORS = ['bg-primary', 'bg-info', 'bg-success', 'bg-warning', 'bg-tag-purple', 'bg-tag-pink'];

	function reload(): void {
		void usage.refresh();
		void devices.refresh();
	}

	/** Start a streamed disk job, show its output and refresh afterwards. */
	async function runJob(reason: string, start: () => Promise<string>): Promise<void> {
		try {
			const id = await sudo.describe(reason, start);
			job = jobs.get(id);
			await job.wait();
			reload();
		} catch (error) {
			toast.error(error);
		}
	}

	async function mount(): Promise<void> {
		const device = mounting;
		if (!device) return;
		try {
			await sudo.describe(`Mount ${device.path}`, () =>
				api.diskMount(device.path, mountPoint.trim(), createDir)
			);
			toast.success(`${device.path} mounted at ${mountPoint.trim()}`);
			mounting = null;
			reload();
		} catch (error) {
			toast.error(error, 'Could not mount');
		}
	}

	async function unmount(device: BlockDevice): Promise<void> {
		const ok = await confirm({
			title: `Unmount ${device.mount}?`,
			message: 'Programs using files on this filesystem will lose access to them.',
			confirmLabel: 'Unmount',
			destructive: true
		});
		if (!ok) return;
		try {
			await sudo.describe(`Unmount ${device.mount}`, () => api.diskUnmount(device.mount));
			toast.success(`${device.mount} unmounted`);
			reload();
		} catch (error) {
			toast.error(error, 'Could not unmount');
		}
	}

	async function expand(device: BlockDevice): Promise<void> {
		const ok = await confirm({
			title: `Expand ${device.path}?`,
			message:
				'The partition is grown to use the free space behind it, then the filesystem is resized. Make sure you have a backup.',
			acknowledge: 'I understand that resizing partitions can cause data loss if it is interrupted.',
			confirmLabel: 'Expand',
			destructive: true
		});
		if (ok) await runJob(`Expand ${device.path}`, () => api.diskExpand(device.path));
	}

	async function fsck(device: BlockDevice): Promise<void> {
		const ok = await confirm({
			title: `Check filesystem on ${device.path}?`,
			message: device.mount
				? 'The filesystem is mounted, so it is only inspected; nothing is repaired.'
				: 'The filesystem is not mounted: errors that are found will be repaired.',
			confirmLabel: 'Run check'
		});
		if (ok) await runJob(`Check ${device.path}`, () => api.diskFsck(device.path));
	}

	async function createPartition(): Promise<void> {
		const disk = partitioning;
		if (!disk) return;
		const wipes = partForm.mode !== 'free';
		const ok = await confirm({
			title: wipes ? `Erase ${disk.path} and create a partition?` : `Create a partition on ${disk.path}?`,
			message: wipes
				? `A new partition table will be written to ${disk.path}. EVERYTHING on this disk (${formatBytes(disk.size)}) will be lost.`
				: 'A new partition is created in the largest unallocated area and formatted.',
			acknowledge: wipes
				? 'I understand that all data on this disk will be destroyed.'
				: 'I understand that this changes the partition table.',
			typeToConfirm: wipes ? disk.name : undefined,
			confirmLabel: 'Create partition',
			destructive: true
		});
		if (!ok) return;
		partitioning = null;
		const table: TableKind | null = partForm.mode === 'free' ? null : partForm.mode;
		await runJob(`Create partition on ${disk.path}`, () =>
			api.diskCreatePartition({
				disk: disk.path,
				newTable: table,
				filesystem: partForm.filesystem,
				label: partForm.label.trim()
			})
		);
	}

	export const refresh = reload;
</script>

<Page>
	{#snippet toolbar()}
		<label class="flex items-center gap-1.5 text-xs text-muted-foreground"
			><Switch size="sm" bind:checked={showSystem} /> System filesystems</label
		>
		<label class="flex items-center gap-1.5 text-xs text-muted-foreground"
			><Switch size="sm" bind:checked={showLoop} /> Loop devices</label
		>
		<span class="flex-1"></span>
		<RefreshControl onrefresh={reload} loading={usage.loading || devices.loading} />
	{/snippet}

	<h3 class="mb-2 text-sm font-semibold">Filesystem usage</h3>
	{#if usage.error && !usage.loaded}
		<StateView kind="error" error={usage.error} onretry={usage.refresh} compact />
	{:else if !usage.loaded}
		<StateView kind="loading" compact />
	{:else}
		<div class="grid grid-cols-[repeat(auto-fill,minmax(17rem,1fr))] gap-3">
			{#each filesystems as fs (fs.mount + fs.device)}
				<article class="rounded-xl border bg-card p-3">
					<div class="flex items-baseline gap-2">
						<h4 class="selectable min-w-0 flex-1 truncate font-mono text-sm font-medium">{fs.mount}</h4>
						<span class="text-xs text-muted-foreground">{fs.fsType}</span>
					</div>
					<p class="selectable truncate font-mono text-[11px] text-muted-foreground">{fs.device}</p>
					<div class="mt-2 h-2 overflow-hidden rounded-full bg-muted">
						<div
							class={cn(
								'h-full rounded-full',
								fs.usePercent > 85 ? 'bg-destructive' : fs.usePercent > 65 ? 'bg-warning' : 'bg-primary'
							)}
							style="width: {fs.usePercent}%"
						></div>
					</div>
					<p class="mt-1.5 flex justify-between text-xs tabular">
						<span>{formatBytes(fs.used)} of {formatBytes(fs.total)}</span>
						<span class="text-muted-foreground"
							>{Math.round(fs.usePercent)}% · {formatBytes(fs.available)} free</span
						>
					</p>
				</article>
			{:else}
				<p class="text-sm text-muted-foreground">No filesystems to show.</p>
			{/each}
		</div>
	{/if}

	<h3 class="mt-6 mb-2 text-sm font-semibold">Block devices</h3>
	<DependencyGuard tools={['lsblk']} inline active={visible}>
		{#if devices.error && !devices.loaded}
			<StateView kind="error" error={devices.error} onretry={devices.refresh} compact />
		{:else if !devices.loaded}
			<StateView kind="loading" compact />
		{:else}
			<div class="flex flex-col gap-3">
				{#each disks as disk (disk.name)}
					{@const parts = partitionsOf(disk.name)}
					{@const free = unallocated(disk)}
					<article class="rounded-xl border bg-card p-3">
						<div class="flex items-center gap-2">
							<HardDrive class="size-4 text-muted-foreground" />
							<h4 class="selectable font-mono text-sm font-medium">{disk.path}</h4>
							<span class="text-xs text-muted-foreground">
								{disk.model || disk.kind} · {formatBytes(disk.size)}{disk.partitionTable
									? ` · ${disk.partitionTable.toUpperCase()}`
									: ''}
							</span>
							<span class="flex-1"></span>
							{#if disk.kind === 'disk'}
								<Button
									variant="outline"
									size="xs"
									onclick={() => {
										partForm = { mode: disk.partitionTable ? 'free' : 'gpt', filesystem: 'ext4', label: '' };
										partitioning = disk;
									}}
								>
									Create partition
								</Button>
							{/if}
						</div>

						{#if disk.kind === 'disk' && disk.size > 0}
							<div
								class="mt-2 flex h-5 overflow-hidden rounded-md bg-muted"
								role="img"
								aria-label="Partition layout"
							>
								{#each parts as part, i (part.name)}
									<div
										class={cn('h-full border-r border-background', COLORS[i % COLORS.length])}
										style="width: {(part.size / disk.size) * 100}%"
										title="{part.path} · {formatBytes(part.size)}"
									></div>
								{/each}
							</div>
							{#if free > disk.size * 0.01}
								<p class="mt-1 text-xs text-muted-foreground">{formatBytes(free)} unallocated</p>
							{/if}
						{/if}

						{#if childrenOf(disk.name).length > 0 || disk.fsType}
							<table class="mt-2 w-full text-sm">
								<thead class="text-xs text-muted-foreground">
									<tr>
										<th class="pb-1 text-left font-medium">Device</th>
										<th class="pb-1 text-right font-medium">Size</th>
										<th class="pb-1 pl-4 text-left font-medium">Filesystem</th>
										<th class="pb-1 text-left font-medium">Mount point</th>
										<th class="pb-1 text-left font-medium">Label</th>
										<th class="pb-1 text-left font-medium">UUID</th>
										<th></th>
									</tr>
								</thead>
								<tbody>
									{#each childrenOf(disk.name).length > 0 ? childrenOf(disk.name) : [disk] as part (part.name)}
										<tr class="border-t">
											<td
												class="selectable py-1.5 font-mono text-xs"
												style="padding-left: {(part.depth - 1) * 16}px"
											>
												{part.path}
												{#if part.kind !== 'part' && part.kind !== 'disk'}<span class="text-muted-foreground"
														>({part.kind})</span
													>{/if}
											</td>
											<td class="py-1.5 text-right text-xs tabular">{formatBytes(part.size)}</td>
											<td class="py-1.5 pl-4 text-xs">{part.fsType || '—'}</td>
											<td class="selectable py-1.5 font-mono text-xs">{part.mount || '—'}</td>
											<td class="py-1.5 text-xs">{part.label || '—'}</td>
											<td class="py-1.5 font-mono text-[11px]">
												{#if part.uuid}
													<button
														type="button"
														class="flex items-center gap-1 text-muted-foreground hover:text-foreground"
														title="Copy UUID"
														onclick={() => copyText(part.uuid).then(() => toast.success('UUID copied'))}
													>
														<span class="max-w-28 truncate">{part.uuid}</span><Copy class="size-3" />
													</button>
												{:else}—{/if}
											</td>
											<td class="py-1 text-right">
												<DropdownMenu.Root>
													<DropdownMenu.Trigger>
														{#snippet child({ props })}<Button {...props} variant="ghost" size="xs"
																>Actions</Button
															>{/snippet}
													</DropdownMenu.Trigger>
													<DropdownMenu.Content align="end">
														{#if part.mount && part.mount !== '[SWAP]'}
															<DropdownMenu.Item disabled={part.mount === '/'} onclick={() => unmount(part)}
																>Unmount</DropdownMenu.Item
															>
														{:else if part.fsType && part.fsType !== 'swap' && part.fsType !== 'LVM2_member'}
															<DropdownMenu.Item
																onclick={() => {
																	mountPoint = `/mnt/${part.label || part.name}`;
																	createDir = true;
																	mounting = part;
																}}>Mount…</DropdownMenu.Item
															>
														{/if}
														{#if part.kind === 'part'}
															<DropdownMenu.Item onclick={() => expand(part)}
																>Expand to fill disk…</DropdownMenu.Item
															>
														{/if}
														{#if part.fsType && part.fsType !== 'swap'}
															<DropdownMenu.Item onclick={() => fsck(part)}
																>Check filesystem…</DropdownMenu.Item
															>
														{/if}
													</DropdownMenu.Content>
												</DropdownMenu.Root>
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						{/if}
					</article>
				{:else}
					<p class="text-sm text-muted-foreground">No block devices reported.</p>
				{/each}
			</div>
		{/if}
	</DependencyGuard>
</Page>

<JobDialog bind:job />

{#if mounting}
	<Modal open title="Mount {mounting.path}" size="md" onclose={() => (mounting = null)}>
		<div class="flex flex-col gap-3">
			<Field label="Mount point" required><PathInput bind:value={mountPoint} dirsOnly /></Field>
			<label class="flex items-center gap-2 text-sm"
				><Checkbox bind:checked={createDir} /> Create the directory if it does not exist</label
			>
			<p class="text-xs text-muted-foreground">
				This mount lasts until the next reboot. Add an entry to /etc/fstab to make it permanent.
			</p>
		</div>
		{#snippet footer()}
			<Button variant="outline" onclick={() => (mounting = null)}>Cancel</Button>
			<Button disabled={!mountPoint.trim().startsWith('/')} onclick={mount}>Mount</Button>
		{/snippet}
	</Modal>
{/if}

{#if partitioning}
	<Modal open title="Create partition on {partitioning.path}" size="md" onclose={() => (partitioning = null)}>
		<DependencyGuard tools={['parted']} inline>
			<div class="flex flex-col gap-3">
				<Field label="Where">
					<SelectField
						bind:value={partForm.mode}
						options={[
							{
								value: 'free',
								label: 'Use the largest unallocated area',
								disabled: !partitioning.partitionTable
							},
							{ value: 'gpt', label: 'Erase the disk: new GPT table, one partition' },
							{ value: 'mbr', label: 'Erase the disk: new MBR table, one partition' }
						]}
					/>
				</Field>
				<Field label="Filesystem">
					<SelectField
						bind:value={partForm.filesystem}
						options={[
							{ value: 'ext4', label: 'ext4' },
							{ value: 'xfs', label: 'XFS' },
							{ value: 'fat32', label: 'FAT32' }
						]}
					/>
				</Field>
				<Field label="Label" hint="Optional."
					><Input bind:value={partForm.label} spellcheck="false" maxlength={16} /></Field
				>
			</div>
		</DependencyGuard>
		{#snippet footer()}
			<Button variant="outline" onclick={() => (partitioning = null)}>Cancel</Button>
			<Button variant="destructive" onclick={createPartition}>Continue…</Button>
		{/snippet}
	</Modal>
{/if}
