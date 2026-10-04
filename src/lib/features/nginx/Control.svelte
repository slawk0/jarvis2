<script lang="ts">
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Page from '$lib/components/Page.svelte';
	import { Button } from '$lib/components/ui/button';
	import { api, type NginxControl, type NginxTarget } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';

	let { visible, target }: { visible: boolean; target: NginxTarget } = $props();

	let output = $state('');
	let ok = $state<boolean | null>(null);
	let running = $state<NginxControl | null>(null);

	async function run(action: NginxControl): Promise<void> {
		if (action === 'restart') {
			const yes = await confirm({
				title: 'Restart nginx?',
				message:
					'Open connections are dropped while nginx restarts. A reload applies configuration changes without that.',
				confirmLabel: 'Restart nginx',
				destructive: true
			});
			if (!yes) return;
		}
		running = action;
		try {
			const result = await sudo.describe(`nginx ${action}`, () => api.nginxControl(target, action));
			output = result.output;
			ok = result.ok;
		} catch (error) {
			toast.error(error);
		} finally {
			running = null;
		}
	}

	let started = false;
	$effect(() => {
		if (visible && !started) {
			started = true;
			void run('status');
		}
	});

	export const refresh = () => run('status');
</script>

<Page scroll={false}>
	{#snippet toolbar()}
		<Button variant="outline" size="sm" disabled={running !== null} onclick={() => run('test')}
			>Test configuration</Button
		>
		<Button variant="outline" size="sm" disabled={running !== null} onclick={() => run('reload')}
			>Reload</Button
		>
		<Button variant="outline" size="sm" disabled={running !== null} onclick={() => run('restart')}
			>Restart</Button
		>
		<Button variant="outline" size="sm" disabled={running !== null} onclick={() => run('status')}
			>Status</Button
		>
		{#if running}
			<span class="text-xs text-muted-foreground">Running…</span>
		{:else if ok !== null}
			<span class={ok ? 'text-xs text-success' : 'text-xs text-destructive'} role="status"
				>{ok ? 'Succeeded' : 'Failed'}</span
			>
		{/if}
	{/snippet}
	<LogViewer
		source={output}
		severity
		placeholder="Run an action to see its output."
		downloadName="nginx.txt"
		class="flex-1"
	/>
</Page>
