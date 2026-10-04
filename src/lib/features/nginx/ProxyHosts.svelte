<script lang="ts">
	import ArrowRight from '@lucide/svelte/icons/arrow-right';
	import Lock from '@lucide/svelte/icons/lock';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import StateView from '$lib/components/StateView.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import { Textarea } from '$lib/components/ui/textarea';
	import { api, apiQuiet, toIpcError, type Certificate, type NginxTarget, type ProxyHost } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';
	import { cn, uid } from '$lib/utils';

	let { visible, target }: { visible: boolean; target: NginxTarget } = $props();

	const hosts = resource((io) => io.nginxHosts(target));
	const busy = new Busy();
	let draft = $state<ProxyHost | null>(null);
	let isNew = $state(false);
	let domainsText = $state('');
	let tab = $state<'details' | 'ssl' | 'advanced'>('details');
	let certificates = $state<Certificate[]>([]);
	let saving = $state(false);
	let testError = $state<string | null>(null);

	autoLoad(hosts, () => visible);

	function blank(): ProxyHost {
		return {
			id: uid().replace(/-/g, '').slice(0, 12),
			domains: [],
			scheme: 'http',
			forwardHost: '127.0.0.1',
			forwardPort: 8080,
			websockets: false,
			blockExploits: true,
			cacheAssets: false,
			ssl: {
				enabled: false,
				certificate: '',
				forceHttps: true,
				http2: true,
				hsts: false,
				hstsSubdomains: false,
				hstsPreload: false
			},
			advanced: '',
			enabled: true
		};
	}

	function open(host: ProxyHost | null): void {
		isNew = host === null;
		draft = structuredClone($state.snapshot(host ?? blank()));
		domainsText = draft.domains.join(', ');
		tab = 'details';
		testError = null;
		apiQuiet.nginxCertificates(target).then(
			(list) => (certificates = list),
			() => (certificates = [])
		);
	}

	const domains = $derived(
		domainsText
			.split(/[\s,]+/)
			.map((d) => d.trim())
			.filter(Boolean)
	);
	const error = $derived.by(() => {
		if (!draft) return null;
		if (domains.length === 0) return 'Enter at least one domain name.';
		if (!draft.forwardHost.trim()) return 'Enter the host to forward to.';
		if (!(draft.forwardPort >= 1 && draft.forwardPort <= 65535)) return 'The forward port must be 1–65535.';
		if (draft.ssl.enabled && !draft.ssl.certificate)
			return 'Choose a certificate for SSL (issue one in the SSL certificates tab).';
		return null;
	});

	async function save(): Promise<void> {
		if (!draft || error) return;
		saving = true;
		testError = null;
		try {
			const host = { ...$state.snapshot(draft), domains, forwardPort: Number(draft.forwardPort) };
			await sudo.describe('Save an nginx proxy host', () => api.nginxHostSave(target, host));
			toast.success('Proxy host saved and nginx reloaded');
			draft = null;
			await hosts.refresh();
		} catch (raw) {
			const e = toIpcError(raw);
			if (e.code === 'CONFIG_TEST_FAILED') testError = e.details ?? e.message;
			else toast.error(e, 'Could not save the proxy host');
		} finally {
			saving = false;
		}
	}

	async function toggle(host: ProxyHost, enabled: boolean): Promise<void> {
		await busy.run(
			host.id,
			() =>
				sudo.describe(`${enabled ? 'Enable' : 'Disable'} a proxy host`, () =>
					api.nginxHostSave(target, { ...host, enabled })
				),
			(e) => toast.error(e, e.details ?? undefined)
		);
		await hosts.refresh();
	}

	async function remove(host: ProxyHost): Promise<void> {
		const ok = await confirm({
			title: `Delete proxy host ${host.domains[0]}?`,
			message: 'Its nginx configuration is removed and nginx is reloaded.',
			confirmLabel: 'Delete',
			destructive: true
		});
		if (!ok) return;
		await busy.run(
			host.id,
			() => sudo.describe('Delete an nginx proxy host', () => api.nginxHostDelete(target, host.id)),
			(e) => toast.error(e)
		);
		await hosts.refresh();
	}

	export const refresh = () => hosts.refresh();
</script>

<Page>
	{#snippet toolbar()}
		<p class="mr-auto text-xs text-muted-foreground">
			Reverse-proxy hosts managed by Jarvis. Other nginx configuration is left untouched.
		</p>
		<Button size="sm" onclick={() => open(null)}><Plus /> Add proxy host</Button>
		<RefreshControl onrefresh={hosts.refresh} loading={hosts.loading} />
	{/snippet}
	{#if hosts.error && !hosts.loaded}
		<StateView kind="error" error={hosts.error} onretry={hosts.refresh} />
	{:else if !hosts.loaded}
		<StateView kind="loading" />
	{:else if hosts.data?.length === 0}
		<StateView
			kind="empty"
			title="No proxy hosts yet"
			message="Add one to forward a domain to an application on this server."
		>
			<Button size="sm" onclick={() => open(null)}><Plus /> Add proxy host</Button>
		</StateView>
	{:else}
		<div class="grid grid-cols-[repeat(auto-fill,minmax(22rem,1fr))] gap-3">
			{#each hosts.data ?? [] as host (host.id)}
				<article
					class={cn(
						'flex flex-col gap-2 rounded-xl border bg-card p-3',
						!host.enabled && 'opacity-70',
						busy.has(host.id) && 'pointer-events-none opacity-60'
					)}
				>
					<div class="flex items-start gap-2">
						<div class="selectable min-w-0 flex-1">
							{#each host.domains as domain (domain)}<p class="truncate text-sm font-semibold">
									{domain}
								</p>{/each}
						</div>
						{#if host.ssl.enabled}<span class="flex items-center gap-1 text-xs text-success"
								><Lock class="size-3.5" /> SSL</span
							>{/if}
						<Switch
							size="sm"
							checked={host.enabled}
							onCheckedChange={(v) => toggle(host, v)}
							aria-label="Enabled"
						/>
					</div>
					<p class="selectable flex items-center gap-1.5 font-mono text-xs text-muted-foreground">
						<ArrowRight class="size-3.5" />
						{host.scheme}://{host.forwardHost}:{host.forwardPort}
					</p>
					<div class="flex items-center gap-1">
						<span class="mr-auto text-xs text-muted-foreground"
							>{host.enabled ? 'Enabled' : 'Disabled'}{host.websockets ? ' · websockets' : ''}</span
						>
						<IconButton label="Edit" onclick={() => open(host)}><Pencil /></IconButton>
						<IconButton label="Delete" onclick={() => remove(host)}><Trash2 /></IconButton>
					</div>
				</article>
			{/each}
		</div>
	{/if}
</Page>

{#if draft}
	{@const d = draft}
	<Modal open title={isNew ? 'Add proxy host' : 'Edit proxy host'} size="lg" onclose={() => (draft = null)}>
		<SubTabs
			bind:value={tab}
			class="mb-4"
			items={[
				{ id: 'details', label: 'Details' },
				{ id: 'ssl', label: 'SSL' },
				{ id: 'advanced', label: 'Advanced' }
			]}
		/>
		{#if tab === 'details'}
			<div class="grid grid-cols-[7rem_1fr_7rem] gap-3">
				<Field label="Domain names" required hint="Separate several with commas." class="col-span-3"
					><Input
						bind:value={domainsText}
						placeholder="app.example.com, www.app.example.com"
						spellcheck="false"
						autofocus
					/></Field
				>
				<Field label="Scheme"><SelectField bind:value={d.scheme} options={['http', 'https']} /></Field>
				<Field label="Forward host" required
					><Input bind:value={d.forwardHost} class="font-mono" spellcheck="false" /></Field
				>
				<Field label="Forward port" required
					><Input type="number" min="1" max="65535" bind:value={d.forwardPort} class="font-mono" /></Field
				>
				<label class="col-span-3 flex items-center gap-2 text-sm"
					><Checkbox bind:checked={d.websockets} /> Websockets support</label
				>
				<label class="col-span-3 flex items-center gap-2 text-sm"
					><Checkbox bind:checked={d.blockExploits} /> Block common exploits</label
				>
				<label class="col-span-3 flex items-center gap-2 text-sm"
					><Checkbox bind:checked={d.cacheAssets} /> Cache static assets</label
				>
			</div>
		{:else if tab === 'ssl'}
			<div class="flex flex-col gap-3">
				<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={d.ssl.enabled} /> Enable SSL (HTTPS)</label
				>
				<Field label="Certificate" hint="Certificates are issued in the SSL certificates tab.">
					<SelectField
						bind:value={d.ssl.certificate}
						disabled={!d.ssl.enabled}
						placeholder={certificates.length ? 'Choose a certificate…' : 'No certificates found'}
						options={[
							...certificates.map((c) => ({ value: c.name, label: `${c.name} (${c.domains.join(', ')})` })),
							...(d.ssl.certificate && !certificates.some((c) => c.name === d.ssl.certificate)
								? [{ value: d.ssl.certificate, label: d.ssl.certificate }]
								: [])
						]}
					/>
				</Field>
				<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={d.ssl.forceHttps} disabled={!d.ssl.enabled} /> Force HTTPS (redirect HTTP)</label
				>
				<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={d.ssl.http2} disabled={!d.ssl.enabled} /> HTTP/2</label
				>
				<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={d.ssl.hsts} disabled={!d.ssl.enabled} /> HSTS</label
				>
				<label class="flex items-center gap-2 pl-6 text-sm"
					><Checkbox bind:checked={d.ssl.hstsSubdomains} disabled={!d.ssl.enabled || !d.ssl.hsts} /> Include subdomains</label
				>
				<label class="flex items-center gap-2 pl-6 text-sm"
					><Checkbox bind:checked={d.ssl.hstsPreload} disabled={!d.ssl.enabled || !d.ssl.hsts} /> Preload</label
				>
			</div>
		{:else}
			<Field label="Custom nginx directives" hint="Added inside the location / block.">
				<Textarea
					bind:value={d.advanced}
					rows={9}
					class="font-mono text-xs"
					spellcheck="false"
					placeholder="client_max_body_size 50m;"
				/>
			</Field>
		{/if}
		{#if testError}
			<div class="mt-4 rounded-lg border border-destructive/40 bg-destructive/10 p-3" role="alert">
				<p class="text-sm font-medium">nginx rejected this configuration; nothing was changed.</p>
				<pre
					class="selectable mt-1 max-h-32 overflow-auto text-xs whitespace-pre-wrap text-muted-foreground">{testError}</pre>
			</div>
		{/if}
		{#snippet footer()}
			{#if error}<span class="mr-auto self-center text-xs text-muted-foreground">{error}</span>{/if}
			<Button variant="outline" onclick={() => (draft = null)}>Cancel</Button>
			<Button disabled={saving || error !== null} onclick={save}
				>{saving ? 'Testing and saving…' : 'Save'}</Button
			>
		{/snippet}
	</Modal>
{/if}
