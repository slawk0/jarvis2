<script lang="ts">
	import FileCode from '@lucide/svelte/icons/file-code';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import FileEditor from '$lib/editor/FileEditor.svelte';
	import { api, type NginxTarget } from '$lib/ipc';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, resource } from '$lib/state/resource.svelte';
	import { matches } from '$lib/utils';

	let { visible, target }: { visible: boolean; target: NginxTarget } = $props();

	const files = resource((io) => io.nginxFiles(target));
	let editing = $state<string | null>(null);
	let search = $state('');

	autoLoad(files, () => visible && !editing);

	/** Files grouped by the directory they are in. */
	const groups = $derived.by(() => {
		const map = new Map<string, string[]>();
		for (const path of (files.data ?? []).filter((p) => matches(search, p))) {
			const dir = path.slice(0, path.lastIndexOf('/'));
			map.set(dir, [...(map.get(dir) ?? []), path]);
		}
		return [...map.entries()];
	});

	async function read(path: string): Promise<{ content: string; elevated: boolean }> {
		const content = await sudo.describe('Read an nginx config file', () => api.nginxFileRead(target, path));
		return { content, elevated: target.target.kind === 'host' };
	}

	/** Saving tests the configuration and reloads; a failed test restores the file. */
	async function write(path: string, content: string): Promise<boolean> {
		await sudo.describe('Save an nginx config file', () => api.nginxFileWrite(target, path, content));
		toast.success('Saved; configuration tested and nginx reloaded');
		return true;
	}

	export const refresh = () => files.refresh();
	export function reset(): void {
		editing = null;
	}
</script>

{#if editing}
	{@const path = editing}
	{#key path}
		<FileEditor
			{path}
			language="ini"
			read={() => read(path)}
			write={(content) => write(path, content)}
			onclose={() => (editing = null)}
		/>
	{/key}
{:else}
	<Page>
		{#snippet toolbar()}
			<SearchInput bind:value={search} placeholder="Filter files…" class="w-64" />
			<p class="mr-auto text-xs text-muted-foreground">
				Saving a file runs <code>nginx -t</code> and reloads; a failed test restores the previous content.
			</p>
			<RefreshControl onrefresh={files.refresh} loading={files.loading} />
		{/snippet}
		{#if files.error && !files.loaded}
			<StateView kind="error" error={files.error} onretry={files.refresh} />
		{:else if !files.loaded}
			<StateView kind="loading" />
		{:else}
			{#each groups as [dir, paths] (dir)}
				<h3 class="selectable mt-3 mb-1 font-mono text-xs text-muted-foreground first:mt-0">{dir}</h3>
				<ul class="divide-y rounded-lg border bg-card">
					{#each paths as path (path)}
						<li>
							<button
								type="button"
								class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-muted/50"
								onclick={() => (editing = path)}
							>
								<FileCode class="size-4 text-muted-foreground" />
								<span class="font-mono text-xs">{path.slice(path.lastIndexOf('/') + 1)}</span>
							</button>
						</li>
					{/each}
				</ul>
			{:else}
				<StateView kind="empty" title="No config files found" compact />
			{/each}
		{/if}
	</Page>
{/if}
