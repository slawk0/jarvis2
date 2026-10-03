<!-- Renders a Resource: loading and error placeholders, then the data. -->
<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';
	import type { Resource } from '$lib/state/resource.svelte';
	import StateView from './StateView.svelte';

	interface Props {
		resource: Resource<T>;
		children: Snippet<[T]>;
		loadingTitle?: string;
	}

	let { resource, children, loadingTitle }: Props = $props();
</script>

{#if resource.data !== undefined}
	{@render children(resource.data)}
{:else if resource.error}
	<StateView kind="error" error={resource.error} onretry={resource.refresh} />
{:else}
	<StateView kind="loading" title={loadingTitle} />
{/if}
