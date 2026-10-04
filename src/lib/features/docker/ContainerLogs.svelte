<!-- Live logs of one container (hidden streamed job). -->
<script lang="ts">
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { api } from '$lib/ipc';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { usePane } from '$lib/workspace/workspace.svelte';

	interface Props {
		name: string;
		visible: boolean;
		onclose: () => void;
	}

	let { name, visible, onclose }: Props = $props();

	let tail = $state('500');
	let job = $state<Job | null>(null);
	const pane = usePane();

	$effect(() => {
		if (!visible) return;
		const lines = Number(tail);
		let current: Job | null = null;
		let cancelled = false;
		api.dockerLogsFollow(name, lines).then(
			(id) => {
				current = jobs.get(id);
				if (cancelled) void current.stop();
				else job = current;
			},
			(e) => toast.error(e)
		);
		return () => {
			cancelled = true;
			void current?.stop();
			job = null;
		};
	});

	$effect(() => pane?.pushBack('Back to containers', onclose));
</script>

<div class="flex h-full min-h-0 flex-col gap-2 p-4">
	<div class="flex items-center gap-2">
		<IconButton label="Back to containers" variant="outline" onclick={onclose}><ArrowLeft /></IconButton>
		<h2 class="selectable truncate text-[15px] font-semibold">Logs · {name}</h2>
	</div>
	<LogViewer
		source={job}
		severity
		live
		placeholder="Waiting for log output…"
		downloadName="{name}.log"
		class="flex-1"
	>
		{#snippet toolbar()}
			<SelectField
				size="sm"
				class="mr-1 w-32"
				bind:value={tail}
				options={[
					{ value: '100', label: 'Last 100' },
					{ value: '500', label: 'Last 500' },
					{ value: '2000', label: 'Last 2,000' },
					{ value: '10000', label: 'Last 10,000' }
				]}
			/>
		{/snippet}
	</LogViewer>
</div>
