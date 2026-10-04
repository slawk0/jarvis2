<script lang="ts">
	import Play from '@lucide/svelte/icons/play';
	import Square from '@lucide/svelte/icons/square';
	import DependencyGuard from '$lib/components/DependencyGuard.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Page from '$lib/components/Page.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, toIpcError, type Diagnostic, type IpInfo, type Tool } from '$lib/ipc';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import type { TabProps } from '$lib/workspace/registry';
	import { workspace } from '$lib/workspace/workspace.svelte';

	let { visible }: TabProps = $props();

	type ToolId = 'ping' | 'traceroute' | 'dns' | 'http' | 'mtr' | 'port' | 'ip';
	const TOOLS: { id: ToolId; label: string; placeholder: string; needs: Tool[] }[] = [
		{ id: 'ping', label: 'Ping', placeholder: 'example.com or 203.0.113.10', needs: ['ping'] },
		{ id: 'traceroute', label: 'Traceroute', placeholder: 'example.com', needs: ['traceroute'] },
		{ id: 'dns', label: 'DNS lookup', placeholder: 'example.com', needs: ['dig'] },
		{ id: 'http', label: 'HTTP check', placeholder: 'https://example.com/health', needs: ['curl'] },
		{ id: 'mtr', label: 'MTR', placeholder: 'example.com', needs: ['mtr'] },
		{ id: 'port', label: 'Port check', placeholder: 'example.com', needs: ['netcat'] },
		{ id: 'ip', label: 'IP info', placeholder: '203.0.113.10', needs: [] }
	];

	let tool = $state<ToolId>('ping');
	let target = $state('');
	let record = $state('A');
	let port = $state('443');
	let job = $state<Job | null>(null);
	let info = $state<IpInfo | null>(null);
	let infoError = $state<string | null>(null);
	let looking = $state(false);

	const current = $derived(TOOLS.find((t) => t.id === tool)!);
	const running = $derived(job?.running ?? false);

	// "Look up this IP" from any tab.
	$effect(() => {
		if (!visible) return;
		const request = workspace.takeRequest('netdiag');
		if (!request) return;
		tool = 'ip';
		target = request.ip;
		void run();
	});

	async function run(): Promise<void> {
		const value = target.trim();
		if (!value) return;
		if (tool === 'ip') {
			looking = true;
			infoError = null;
			info = null;
			try {
				info = await api.ipInfo(value);
			} catch (raw) {
				infoError = toIpcError(raw).message;
			} finally {
				looking = false;
			}
			return;
		}
		if (running) return;
		let diagnostic: Diagnostic;
		if (tool === 'dns') diagnostic = { tool: 'dns', record };
		else if (tool === 'port') diagnostic = { tool: 'port', port: Number(port) };
		else diagnostic = { tool };
		try {
			job = jobs.get(await api.netdiagRun(diagnostic, value));
		} catch (error) {
			toast.error(error);
		}
	}

	export function refresh(): void {
		void run();
	}
</script>

<Page scroll={false}>
	{#snippet header()}
		<SubTabs bind:value={tool} items={TOOLS} />
	{/snippet}

	{#key tool}
		<DependencyGuard tools={current.needs} inline active={visible}>
			<form
				class="mb-3 flex flex-wrap items-center gap-2"
				onsubmit={(e) => {
					e.preventDefault();
					void run();
				}}
			>
				<Input
					bind:value={target}
					placeholder={current.placeholder}
					class="w-96 font-mono text-xs"
					spellcheck="false"
					autocomplete="off"
				/>
				{#if tool === 'dns'}
					<SelectField
						bind:value={record}
						class="w-28"
						options={['A', 'AAAA', 'MX', 'TXT', 'NS', 'CNAME', 'SOA', 'PTR']}
					/>
				{:else if tool === 'port'}
					<Input bind:value={port} class="w-24 font-mono text-xs" inputmode="numeric" placeholder="port" />
				{/if}
				{#if running}
					<Button type="button" variant="destructive" onclick={() => job?.stop()}><Square /> Stop</Button>
				{:else}
					<Button type="submit" disabled={!target.trim() || looking}
						><Play /> {tool === 'ip' ? 'Look up' : 'Run'}</Button
					>
				{/if}
				<Button
					type="button"
					variant="outline"
					onclick={() => {
						job = null;
						info = null;
						infoError = null;
					}}>Clear</Button
				>
				<span class="text-xs text-muted-foreground">
					{tool === 'ip'
						? 'Looked up from this computer using ipapi.co.'
						: 'Runs on the server, not on this computer.'}
				</span>
			</form>

			{#if tool === 'ip'}
				{#if infoError}
					<p class="selectable text-sm text-destructive" role="alert">{infoError}</p>
				{:else if looking}
					<p class="text-sm text-muted-foreground">Looking up…</p>
				{:else if info}
					<section class="selectable max-w-xl rounded-xl border bg-card p-4">
						<h3 class="mb-3 font-mono text-sm font-semibold">{info.ip}</h3>
						<dl class="grid grid-cols-[8rem_1fr] gap-x-4 gap-y-1.5 text-sm">
							<dt class="text-muted-foreground">Country</dt>
							<dd>{info.country} {info.countryCode ? `(${info.countryCode})` : ''}</dd>
							<dt class="text-muted-foreground">Region</dt>
							<dd>{info.region || '—'}</dd>
							<dt class="text-muted-foreground">City</dt>
							<dd>{info.city || '—'} {info.postal}</dd>
							<dt class="text-muted-foreground">ISP / org</dt>
							<dd>{info.org || '—'}</dd>
							<dt class="text-muted-foreground">ASN</dt>
							<dd class="font-mono text-xs">{info.asn || '—'}</dd>
							<dt class="text-muted-foreground">Time zone</dt>
							<dd>{info.timezone || '—'}</dd>
							<dt class="text-muted-foreground">Coordinates</dt>
							<dd class="font-mono text-xs">
								{info.latitude && info.longitude ? `${info.latitude}, ${info.longitude}` : '—'}
							</dd>
						</dl>
						<p class="mt-3 text-xs text-muted-foreground">Source: {info.provider}</p>
					</section>
				{:else}
					<p class="text-sm text-muted-foreground">
						Enter an IP address, or right-click any IP in Jarvis and choose “Look up in Net Diagnostics”.
					</p>
				{/if}
			{:else}
				<LogViewer
					source={job}
					live
					placeholder="Enter a target and press Run."
					downloadName="{tool}.txt"
					class="flex-1"
				/>
			{/if}
		</DependencyGuard>
	{/key}
</Page>
