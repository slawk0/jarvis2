<!-- Edit a container = recreate it with new settings (old one restored on failure). -->
<script lang="ts">
	import Plus from '@lucide/svelte/icons/plus';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import X from '@lucide/svelte/icons/x';
	import Field from '$lib/components/Field.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { api, type ContainerDetail, type ContainerSpec } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { toast } from '$lib/services/toast.svelte';

	interface Props {
		detail: ContainerDetail;
		onclose: () => void;
		ondone: (name: string) => void;
	}

	let { detail, onclose, ondone }: Props = $props();

	const mb = (bytes: number) => (bytes ? `${Math.round(bytes / 1024 / 1024)}m` : '');
	const builtin = ['bridge', 'host', 'none'];
	// svelte-ignore state_referenced_locally
	const primary =
		builtin.includes(detail.networkMode) || detail.networkMode === 'default'
			? detail.networkMode === 'default'
				? 'bridge'
				: detail.networkMode
			: (detail.networks[0]?.name ?? detail.networkMode);

	// svelte-ignore state_referenced_locally
	let spec = $state<ContainerSpec>({
		name: detail.name,
		image: detail.image,
		command: detail.commandLine,
		entrypoint: '',
		workingDir: detail.workingDir,
		user: detail.user,
		// A hostname equal to the container id is Docker's default, not a setting.
		hostname: detail.id.startsWith(detail.hostname) ? '' : detail.hostname,
		tty: detail.tty,
		interactive: detail.interactive,
		restartPolicy: detail.restartPolicy,
		env: detail.env.map(([k, v]) => [k, v]),
		labels: detail.labels.filter(([k]) => !k.startsWith('com.docker.compose.')).map(([k, v]) => [k, v]),
		ports: detail.ports.map((p) => ({ ...p })),
		mounts: detail.mounts.map((m) => ({ ...m })),
		network: primary,
		extraNetworks: detail.networks.map((n) => n.name).filter((n) => n !== primary),
		dns: [...detail.dns],
		logDriver: detail.logDriver,
		logOptions: detail.logOptions.map(([k, v]) => [k, v]),
		memory: mb(detail.memory),
		memoryReservation: mb(detail.memoryReservation),
		cpus: detail.nanoCpus ? String(detail.nanoCpus / 1e9) : '',
		shmSize: detail.shmSize && detail.shmSize !== 67108864 ? mb(detail.shmSize) : '',
		privileged: detail.privileged,
		capAdd: [...detail.capAdd],
		capDrop: [...detail.capDrop]
	});

	type Tab = 'general' | 'storage' | 'network' | 'env' | 'runtime';
	let tab = $state<Tab>('general');
	let networks = $state<string[]>(builtin);
	let job = $state<Job | null>(null);
	let submitting = $state(false);

	api.dockerNetworks().then(
		(list) => (networks = [...new Set([...builtin, ...list.map((n) => n.name)])]),
		() => {}
	);

	async function submit(): Promise<void> {
		const ok = await confirm({
			title: `Recreate “${detail.name}”?`,
			message:
				'The container is stopped and replaced by a new one with these settings. Data outside volumes is lost. If creating the new container fails, the current one is restored.',
			confirmLabel: 'Recreate container',
			destructive: true
		});
		if (!ok) return;
		submitting = true;
		try {
			const clean = $state.snapshot(spec);
			clean.capAdd = clean.capAdd.filter(Boolean);
			clean.capDrop = clean.capDrop.filter(Boolean);
			clean.dns = clean.dns.filter(Boolean);
			const id = await api.dockerRecreate(detail.name, clean);
			job = jobs.get(id);
			const result = await job.wait();
			if (result.status === 'done') {
				toast.success('Container recreated');
				ondone(clean.name.trim());
			}
		} catch (error) {
			toast.error(error, 'Could not recreate the container');
		} finally {
			submitting = false;
		}
	}
</script>

{#snippet pairsEditor(pairs: [string, string][], keyLabel: string, add: string)}
	<div class="flex flex-col gap-1.5">
		{#each pairs as pair, i (i)}
			<div class="flex gap-1.5">
				<Input
					bind:value={pair[0]}
					placeholder={keyLabel}
					class="w-1/3 font-mono text-xs"
					spellcheck="false"
				/>
				<Input bind:value={pair[1]} placeholder="value" class="flex-1 font-mono text-xs" spellcheck="false" />
				<Button variant="ghost" size="icon-sm" aria-label="Remove" onclick={() => pairs.splice(i, 1)}
					><X /></Button
				>
			</div>
		{/each}
		<div>
			<Button variant="outline" size="xs" onclick={() => pairs.push(['', ''])}><Plus /> {add}</Button>
		</div>
	</div>
{/snippet}

{#snippet listEditor(items: string[], placeholder: string, add: string)}
	<div class="flex flex-col gap-1.5">
		{#each items as _, i (i)}
			<div class="flex gap-1.5">
				<Input bind:value={items[i]} {placeholder} class="font-mono text-xs" spellcheck="false" />
				<Button variant="ghost" size="icon-sm" aria-label="Remove" onclick={() => items.splice(i, 1)}
					><X /></Button
				>
			</div>
		{/each}
		<div><Button variant="outline" size="xs" onclick={() => items.push('')}><Plus /> {add}</Button></div>
	</div>
{/snippet}

<Modal
	open
	title="Edit container · {detail.name}"
	size="xl"
	class="h-[80vh]"
	dismissible={!submitting}
	{onclose}
>
	{#if job}
		<LogViewer source={job} controls={false} class="h-full min-h-60" />
	{:else}
		{#if detail.composeProject}
			<p
				class="mb-3 flex items-center gap-2 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-xs"
			>
				<TriangleAlert class="size-4 shrink-0 text-warning" />
				This container belongs to Compose project “{detail.composeProject}”. Recreating it here detaches it
				from the stack.
			</p>
		{/if}
		<SubTabs
			bind:value={tab}
			class="mb-4"
			items={[
				{ id: 'general', label: 'General' },
				{ id: 'storage', label: 'Volumes', count: spec.mounts.length },
				{ id: 'network', label: 'Network', count: spec.ports.length },
				{ id: 'env', label: 'Environment', count: spec.env.length },
				{ id: 'runtime', label: 'Runtime & resources' }
			]}
		/>
		{#if tab === 'general'}
			<div class="grid grid-cols-2 gap-3">
				<Field label="Name" required><Input bind:value={spec.name} spellcheck="false" /></Field>
				<Field label="Image" required
					><Input bind:value={spec.image} class="font-mono text-xs" spellcheck="false" /></Field
				>
				<Field label="Command" hint="Empty uses the image default." class="col-span-2"
					><Input bind:value={spec.command} class="font-mono text-xs" spellcheck="false" /></Field
				>
				<Field label="Entrypoint override" hint="Empty keeps the image entrypoint."
					><Input bind:value={spec.entrypoint} class="font-mono text-xs" spellcheck="false" /></Field
				>
				<Field label="Working directory"
					><Input bind:value={spec.workingDir} class="font-mono text-xs" spellcheck="false" /></Field
				>
				<Field label="User"
					><Input bind:value={spec.user} placeholder="uid:gid or name" spellcheck="false" /></Field
				>
				<Field label="Restart policy"
					><SelectField
						bind:value={spec.restartPolicy}
						options={[...new Set(['no', 'always', 'unless-stopped', 'on-failure', spec.restartPolicy])]}
					/></Field
				>
				<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={spec.tty} /> Allocate a TTY (-t)</label
				>
				<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={spec.interactive} /> Keep stdin open (-i)</label
				>
			</div>
		{:else if tab === 'storage'}
			<div class="flex flex-col gap-2">
				{#each spec.mounts as mount, i (i)}
					<div class="flex items-center gap-1.5">
						<SelectField
							size="sm"
							class="w-24"
							bind:value={mount.kind}
							options={['bind', 'volume', 'tmpfs']}
						/>
						{#if mount.kind === 'bind'}
							<PathInput bind:value={mount.source} placeholder="/host/path" class="flex-1" />
						{:else if mount.kind === 'volume'}
							<Input
								bind:value={mount.source}
								placeholder="volume name"
								class="flex-1 font-mono text-xs"
								spellcheck="false"
							/>
						{:else}
							<span class="flex-1 text-xs text-muted-foreground">in-memory filesystem</span>
						{/if}
						<Input
							bind:value={mount.target}
							placeholder="/container/path"
							class="flex-1 font-mono text-xs"
							spellcheck="false"
						/>
						<label class="flex shrink-0 items-center gap-1.5 text-xs"
							><Checkbox bind:checked={mount.readOnly} /> RO</label
						>
						<Button
							variant="ghost"
							size="icon-sm"
							aria-label="Remove"
							onclick={() => spec.mounts.splice(i, 1)}><X /></Button
						>
					</div>
				{/each}
				<div>
					<Button
						variant="outline"
						size="xs"
						onclick={() => spec.mounts.push({ kind: 'bind', source: '', target: '', readOnly: false })}
						><Plus /> Add volume</Button
					>
				</div>
			</div>
		{:else if tab === 'network'}
			<div class="grid grid-cols-2 gap-3">
				<Field label="Primary network"
					><SelectField
						bind:value={spec.network}
						options={[...new Set([...networks, spec.network])]}
					/></Field
				>
				<Field label="Hostname"><Input bind:value={spec.hostname} spellcheck="false" /></Field>
				<Field label="Additional networks" class="col-span-2">
					<div class="flex flex-wrap gap-x-4 gap-y-1.5">
						{#each networks.filter((n) => !builtin.includes(n) && n !== spec.network) as n (n)}
							<label class="flex items-center gap-1.5 text-sm">
								<Checkbox
									checked={spec.extraNetworks.includes(n)}
									onCheckedChange={(v) =>
										(spec.extraNetworks = v
											? [...spec.extraNetworks, n]
											: spec.extraNetworks.filter((x) => x !== n))}
								/>
								{n}
							</label>
						{:else}
							<span class="text-xs text-muted-foreground">No other user-defined networks.</span>
						{/each}
					</div>
				</Field>
				<Field label="Port mappings" class="col-span-2">
					<div class="flex flex-col gap-1.5">
						{#each spec.ports as port, i (i)}
							<div class="flex items-center gap-1.5">
								<Input
									bind:value={port.hostIp}
									placeholder="host IP (any)"
									class="w-40 font-mono text-xs"
									spellcheck="false"
								/>
								<Input bind:value={port.hostPort} placeholder="host port" class="w-28 font-mono text-xs" />
								<span class="text-muted-foreground">→</span>
								<Input
									bind:value={port.containerPort}
									placeholder="container port"
									class="w-32 font-mono text-xs"
								/>
								<SelectField size="sm" class="w-20" bind:value={port.protocol} options={['tcp', 'udp']} />
								<Button
									variant="ghost"
									size="icon-sm"
									aria-label="Remove"
									onclick={() => spec.ports.splice(i, 1)}><X /></Button
								>
							</div>
						{/each}
						<div>
							<Button
								variant="outline"
								size="xs"
								onclick={() =>
									spec.ports.push({ hostIp: '', hostPort: '', containerPort: '', protocol: 'tcp' })}
								><Plus /> Add port</Button
							>
						</div>
					</div>
				</Field>
				<Field label="DNS servers" class="col-span-2"
					>{@render listEditor(spec.dns, '1.1.1.1', 'Add DNS server')}</Field
				>
			</div>
		{:else if tab === 'env'}
			<Field label="Environment variables">{@render pairsEditor(spec.env, 'NAME', 'Add variable')}</Field>
			<Field label="Labels" class="mt-4">{@render pairsEditor(spec.labels, 'label', 'Add label')}</Field>
		{:else}
			<div class="grid grid-cols-2 gap-3">
				<Field label="Memory limit" hint="e.g. 512m, 2g. Empty is unlimited."
					><Input bind:value={spec.memory} class="font-mono text-xs" /></Field
				>
				<Field label="Memory reservation"
					><Input bind:value={spec.memoryReservation} class="font-mono text-xs" /></Field
				>
				<Field label="CPUs" hint="e.g. 1.5"><Input bind:value={spec.cpus} class="font-mono text-xs" /></Field>
				<Field label="Shared memory (/dev/shm)" hint="Default is 64m."
					><Input bind:value={spec.shmSize} class="font-mono text-xs" /></Field
				>
				<Field label="Log driver"
					><Input
						bind:value={spec.logDriver}
						placeholder="json-file"
						class="font-mono text-xs"
						spellcheck="false"
					/></Field
				>
				<label class="flex items-center gap-2 self-end pb-2 text-sm"
					><Checkbox bind:checked={spec.privileged} /> Privileged</label
				>
				<Field label="Log options" class="col-span-2"
					>{@render pairsEditor(spec.logOptions, 'max-size', 'Add log option')}</Field
				>
				<Field label="Add capabilities"
					>{@render listEditor(spec.capAdd, 'NET_ADMIN', 'Add capability')}</Field
				>
				<Field label="Drop capabilities">{@render listEditor(spec.capDrop, 'ALL', 'Drop capability')}</Field>
			</div>
		{/if}
	{/if}
	{#snippet footer()}
		{#if job && !job.running}
			<span class="mr-auto self-center text-xs {job.status === 'done' ? 'text-success' : 'text-destructive'}">
				{job.status === 'done' ? 'Recreated.' : 'Failed; the previous container was restored.'}
			</span>
			{#if job.status !== 'done'}<Button variant="outline" onclick={() => (job = null)}
					>Back to the form</Button
				>{/if}
			<Button onclick={onclose}>Close</Button>
		{:else}
			<Button variant="outline" disabled={submitting} onclick={onclose}>Cancel</Button>
			<Button disabled={submitting || !spec.name.trim() || !spec.image.trim()} onclick={submit}
				>{submitting ? 'Recreating…' : 'Recreate container'}</Button
			>
		{/if}
	{/snippet}
</Modal>
