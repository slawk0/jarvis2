<script lang="ts">
	import StateView from '$lib/components/StateView.svelte';
	import type { TabProps } from '$lib/workspace/registry';

	let { visible: _visible, profile: _profile }: TabProps = $props();

	export function refresh(): void {}
</script>

<StateView kind="empty" title="STUB" />
