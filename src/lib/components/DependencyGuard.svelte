<!--
	Wraps content that needs server-side tools. States: checking → ready |
	missing (a card per tool with install, re-check, manual command, docs) |
	error with retry.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import Copy from '@lucide/svelte/icons/copy';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import PackageIcon from '@lucide/svelte/icons/package';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import { Button } from '$lib/components/ui/button';
	import { api, type DepsReport, type IpcError, type Tool, toIpcError } from '$lib/ipc';
	import { app } from '$lib/services/app.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { copyText } from '$lib/utils';
	import LogViewer from './LogViewer.svelte';
	import StateView from './StateView.svelte';

	interface Props {
		tools: Tool[];
		/** Check only once the content is actually needed. */
		active?: boolean;
		/** Render inline (single tool inside a tab) instead of filling the pane. */
		inline?: boolean;
		children: Snippet;
	}

	let { tools, active = true, inline = false, children }: Props = $props();

	let report = $state<DepsReport | null>(null);
	let error = $state<IpcError | null>(null);
	let checking = $state(false);
	let installing = $state<Tool | null>(null);
	let installJob = $state<Job | null>(null);
	let checked = false;

	const missing = $derived(report?.tools.filter((t) => !t.installed) ?? []);

	async function check(): Promise<void> {
		checking = true;
		error = null;
		try {
			report = await api.depsCheck(tools);
		} catch (raw) {
			error = toIpcError(raw);
		} finally {
			checking = false;
		}
	}

	$effect(() => {
		if (active && app.online && !checked) {
			checked = true;
			void check();
		}
	});

	async function install(tool: Tool): Promise<void> {
		installing = tool;
		installJob = null;
		try {
			await jobs.run(() => api.depsInstall(tool), { onStart: (job) => (installJob = job) });
			toast.success('Installed');
			installJob = null;
		} catch (raw) {
			toast.error(raw, 'Installation failed');
		} finally {
			installing = null;
			await check();
		}
	}
</script>

{#if tools.length === 0 || (report && missing.length === 0)}
	{@render children()}
{:else if error}
	<StateView kind="error" {error} onretry={check} compact={inline} />
{:else if !report}
	<StateView kind="loading" title="Checking requirements…" compact={inline} />
{:else}
	<div class={inline ? 'flex flex-col gap-3' : 'mx-auto flex h-full max-w-2xl flex-col justify-center gap-3 p-6'}>
		{#if !inline}
			<div>
				<h2 class="text-base font-semibold">Missing requirements</h2>
				<p class="text-muted-foreground text-sm">
					This feature needs the following on the server.
					{#if !report.packageManager}
						No supported package manager was detected, so automatic installation may not be available.
					{/if}
				</p>
			</div>
		{/if}
		{#each missing as tool (tool.tool)}
			<div class="bg-card flex flex-col gap-2 rounded-lg border p-3">
				<div class="flex items-center gap-2">
					<PackageIcon class="text-muted-foreground size-4" />
					<span class="text-sm font-medium">{tool.label}</span>
					<span class="text-muted-foreground text-xs">not installed</span>
					<div class="ml-auto flex items-center gap-1.5">
						<Button variant="ghost" size="sm" onclick={() => openUrl(tool.docsUrl)}>
							<ExternalLink /> Docs
						</Button>
						{#if tool.installable && tool.manualCommand}
							<Button size="sm" disabled={installing !== null} onclick={() => install(tool.tool)}>
								{installing === tool.tool ? 'Installing…' : `Install ${tool.label}`}
							</Button>
						{/if}
					</div>
				</div>
				{#if !tool.installable}
					<p class="text-muted-foreground text-xs">
						{tool.label} cannot be installed by Jarvis; this server does not provide it.
					</p>
				{:else if tool.manualCommand}
					<div class="bg-sunken flex items-start gap-2 rounded-md border px-2 py-1.5">
						<code class="selectable text-muted-foreground min-w-0 flex-1 text-[11px] break-all">
							{tool.manualCommand}
						</code>
						<button
							type="button"
							class="text-muted-foreground hover:text-foreground shrink-0"
							aria-label="Copy command"
							onclick={() => copyText(tool.manualCommand ?? '').then(() => toast.success('Command copied'))}
						>
							<Copy class="size-3.5" />
						</button>
					</div>
				{:else}
					<p class="text-muted-foreground text-xs">
						No package manager was detected. Install it manually, then re-check.
					</p>
				{/if}
				{#if installing === tool.tool && installJob}
					<LogViewer source={installJob} controls={false} class="h-48" />
				{/if}
			</div>
		{/each}
		<div>
			<Button variant="outline" size="sm" disabled={checking || installing !== null} onclick={check}>
				<RefreshCw class={checking ? 'animate-spin' : ''} /> Re-check
			</Button>
		</div>
	</div>
{/if}
