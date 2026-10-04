<script lang="ts">
	import Braces from '@lucide/svelte/icons/braces';
	import Download from '@lucide/svelte/icons/download';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column, type Sort } from '$lib/components/DataTable.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import JobDialog from '$lib/components/JobDialog.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import { Button } from '$lib/components/ui/button';
	import { api, type Image } from '$lib/ipc';
	import { confirm, prompt } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';

	let { visible, onchanged }: { visible: boolean; onchanged: () => void } = $props();

	const images = resource((io) => io.dockerImages());
	const busy = new Busy();
	let search = $state('');
	let sort = $state<Sort | null>({ key: 'repository', dir: 'asc' });
	let selected = $state(new Set<string>());
	let job = $state<Job | null>(null);
	let inspect = $state<{ name: string; json: string } | null>(null);

	autoLoad(images, () => visible);

	const label = (i: Image) =>
		i.repository === '<none>' ? i.id.replace('sha256:', '').slice(0, 12) : `${i.repository}:${i.tag}`;
	/** Tagged images are removed by name so other tags of the same id survive. */
	const ref = (i: Image) =>
		i.repository === '<none>' || i.tag === '<none>' ? i.id : `${i.repository}:${i.tag}`;
	const key = (i: Image) => `${i.id}|${i.repository}|${i.tag}`;

	const columns: Column<Image>[] = [
		{ key: 'repository', label: 'Repository', value: (i) => i.repository, class: 'max-w-0 w-full' },
		{ key: 'tag', label: 'Tag', value: (i) => i.tag, class: 'w-44 max-w-44' },
		{
			key: 'id',
			label: 'ID',
			value: (i) => i.id.replace('sha256:', '').slice(0, 12),
			mono: true,
			class: 'w-32'
		},
		{ key: 'size', label: 'Size', value: (i) => i.size, align: 'right', class: 'w-24 tabular' },
		{ key: 'created', label: 'Created', value: (i) => i.created, class: 'w-36' }
	];

	async function remove(list: Image[]): Promise<void> {
		const ok = await confirm({
			title: list.length === 1 ? `Remove image ${label(list[0])}?` : `Remove ${list.length} images?`,
			detail: list.length > 1 ? list.map(label).join('\n') : undefined,
			message: list.some((i) => !i.unused)
				? 'Some of these images are used by containers; removing them is forced.'
				: undefined,
			confirmLabel: 'Remove',
			destructive: true
		});
		if (!ok) return;
		const done = await busy.run(
			list.map(key),
			() =>
				api.dockerImageRemove(
					list.map(ref),
					list.some((i) => !i.unused)
				),
			(e) => toast.error(e)
		);
		if (done) toast.success('Image removed');
		selected = new Set();
		await images.refresh();
		onchanged();
	}

	async function pull(): Promise<void> {
		const name = await prompt({
			title: 'Pull image',
			label: 'Image',
			placeholder: 'nginx:latest',
			validate: (v) => (v.trim() ? null : 'Enter an image name.')
		});
		if (!name) return;
		try {
			job = jobs.get(await api.dockerImagePull(name.trim()));
			await job.wait();
			await images.refresh();
			onchanged();
		} catch (error) {
			toast.error(error);
		}
	}

	async function prune(): Promise<void> {
		const ok = await confirm({
			title: 'Remove all unused images?',
			message: 'Every image that no container uses is deleted.',
			confirmLabel: 'Prune images',
			destructive: true
		});
		if (!ok) return;
		try {
			const summary = await api.dockerPrune('images');
			toast.success(summary.trim().split('\n').pop() ?? 'Unused images removed');
			await images.refresh();
			onchanged();
		} catch (error) {
			toast.error(error);
		}
	}

	async function showInspect(image: Image): Promise<void> {
		try {
			inspect = { name: label(image), json: await api.dockerInspect('image', image.id) };
		} catch (error) {
			toast.error(error);
		}
	}

	export const refresh = () => images.refresh();
	/** Quiet reload after something changed outside this view. */
	export const sync = () => images.load(true);
</script>

<Page scroll={false}>
	{#snippet toolbar()}
		<SearchInput bind:value={search} placeholder="Search images…" class="w-64" />
		<span class="flex-1"></span>
		<Button variant="outline" size="sm" onclick={prune}>Prune unused</Button>
		<Button size="sm" onclick={pull}><Download /> Pull image</Button>
		<RefreshControl onrefresh={images.refresh} loading={images.loading} />
	{/snippet}
	<DataTable
		rows={images.data ?? []}
		{columns}
		rowKey={key}
		{search}
		bind:sort
		selectable
		bind:selected
		busy={busy.keys}
		loading={images.loading}
		error={images.error}
		onretry={images.refresh}
		empty="No images"
		class="flex-1"
	>
		{#snippet cell(image, column)}
			{#if column.key === 'repository'}
				<span class="flex items-center gap-2">
					<span class="truncate font-mono text-xs">{image.repository}</span>
					{#if image.unused}<span class="shrink-0 rounded bg-muted px-1 text-[10px] text-muted-foreground"
							>unused</span
						>{/if}
				</span>
			{:else}
				{column.value?.(image)}
			{/if}
		{/snippet}
		{#snippet actions(image)}
			<IconButton label="Inspect" onclick={() => showInspect(image)}><Braces /></IconButton>
			<IconButton label="Remove" onclick={() => remove([image])}><Trash2 /></IconButton>
		{/snippet}
		{#snippet bulk(rows)}
			<Button variant="destructive" size="xs" onclick={() => remove(rows)}>Remove</Button>
		{/snippet}
	</DataTable>
</Page>

<JobDialog bind:job />
{#if inspect}
	<Modal
		open
		title="Inspect · {inspect.name}"
		size="xl"
		class="h-[75vh]"
		flush
		onclose={() => (inspect = null)}
	>
		<LogViewer source={inspect.json} downloadName="image.json" class="flex-1" />
	</Modal>
{/if}
