<script lang="ts">
	import '../app.css';
	import * as Tooltip from '$lib/components/ui/tooltip';

	let { children } = $props();
</script>

<Tooltip.Provider delayDuration={350} disableHoverableContent>
	{@render children()}
</Tooltip.Provider>
