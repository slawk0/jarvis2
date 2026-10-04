<script lang="ts">
	import type { TabProps } from '$lib/workspace/registry';
	import { workspace } from '$lib/workspace/workspace.svelte';
	import FileBrowser from './FileBrowser.svelte';

	let { visible }: TabProps = $props();
	let browser = $state<FileBrowser | null>(null);

	// Other tabs can ask to show a folder.
	$effect(() => {
		if (!visible || !browser) return;
		const request = workspace.takeRequest('files');
		if (request) browser.goTo(request.path);
	});

	export function refresh(): void {
		void browser?.refresh();
	}

	export function onReselect(): void {
		browser?.closeEditor();
	}
</script>

<FileBrowser bind:this={browser} {visible} />
