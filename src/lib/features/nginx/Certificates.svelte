<script lang="ts">
	import Plus from '@lucide/svelte/icons/plus';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import DependencyGuard from '$lib/components/DependencyGuard.svelte';
	import Field from '$lib/components/Field.svelte';
	import JobDialog from '$lib/components/JobDialog.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		api,
		type CertAction,
		type Certificate,
		type IssueMethod,
		type NginxTarget,
		type Tool
	} from '$lib/ipc';
	import { app } from '$lib/services/app.svelte';
	import { confirm } from '$lib/services/confirm.svelte';
	import { jobs, type Job } from '$lib/services/jobs.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';

	let { visible, target, host }: { visible: boolean; target: NginxTarget; host: boolean } = $props();

	const certs = resource((io) => io.nginxCertificates(target));
	let job = $state<Job | null>(null);
	let issueOpen = $state(false);
	let form = $state({
		domains: '',
		email: '',
		method: 'nginx',
		webroot: '/var/www/html',
		token: '',
		accessKey: '',
		secretKey: ''
	});
	let stored = $state<Record<string, boolean>>({});

	autoLoad(certs, () => visible);

	const METHODS = [
		{ value: 'nginx', label: 'Nginx plugin (recommended)' },
		{ value: 'webroot', label: 'Webroot (files served by a running site)' },
		{ value: 'standalone', label: 'Standalone (port 80 must be free)' },
		{ value: 'dnsCloudflare', label: 'DNS challenge · Cloudflare' },
		{ value: 'dnsDigitalocean', label: 'DNS challenge · DigitalOcean' },
		{ value: 'dnsRoute53', label: 'DNS challenge · AWS Route 53' }
	];
	const PLUGIN: Record<string, Tool[]> = {
		nginx: ['certbotNginx'],
		dnsCloudflare: ['certbotDnsCloudflare'],
		dnsDigitalocean: ['certbotDnsDigitalocean'],
		dnsRoute53: ['certbotDnsRoute53']
	};
	const SECRET: Record<string, string> = {
		dnsCloudflare: 'nginx/dns/cloudflare-token',
		dnsDigitalocean: 'nginx/dns/digitalocean-token'
	};

	const columns: Column<Certificate>[] = [
		{ key: 'name', label: 'Certificate', value: (c) => c.name, class: 'w-64 max-w-64' },
		{ key: 'domains', label: 'Domains', value: (c) => c.domains.join(', '), class: 'max-w-0 w-full' },
		{ key: 'expiry', label: 'Expires', value: (c) => c.expiry, class: 'w-56' },
		{ key: 'daysLeft', label: 'Days left', value: (c) => c.daysLeft, align: 'right', class: 'w-28 tabular' }
	];

	async function run(start: () => Promise<string>, reason: string): Promise<void> {
		try {
			job = jobs.get(await sudo.describe(reason, start));
			await job.wait();
			await certs.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	const act = (action: CertAction, reason: string) => run(() => api.nginxCertAction(target, action), reason);

	async function remove(cert: Certificate): Promise<void> {
		const ok = await confirm({
			title: `Delete certificate ${cert.name}?`,
			message: 'Proxy hosts that still use this certificate will stop working until you choose another one.',
			confirmLabel: 'Delete certificate',
			destructive: true
		});
		if (ok) await act({ action: 'delete', name: cert.name }, `Delete certificate ${cert.name}`);
	}

	async function openIssue(): Promise<void> {
		form = { ...form, domains: '', token: '', accessKey: '', secretKey: '' };
		const profile = app.requireProfileId();
		const names = [...Object.values(SECRET), 'nginx/dns/route53-access-key', 'nginx/dns/route53-secret-key'];
		const flags = await Promise.all(names.map((n) => api.secretExists(profile, n).catch(() => false)));
		stored = Object.fromEntries(names.map((n, i) => [n, flags[i]]));
		issueOpen = true;
	}

	const domains = $derived(form.domains.split(/[\s,]+/).filter(Boolean));
	const dnsReady = $derived.by(() => {
		if (form.method === 'dnsRoute53') {
			return (
				(form.accessKey || stored['nginx/dns/route53-access-key']) &&
				(form.secretKey || stored['nginx/dns/route53-secret-key'])
			);
		}
		const name = SECRET[form.method];
		return !name || form.token || stored[name];
	});
	const issueError = $derived(
		domains.length === 0
			? 'Enter at least one domain.'
			: !/^\S+@\S+\.\S+$/.test(form.email.trim())
				? 'Enter a contact email address.'
				: !dnsReady
					? 'Enter the provider credentials.'
					: null
	);

	async function issue(): Promise<void> {
		if (issueError) return;
		const profile = app.requireProfileId();
		try {
			// Tokens go to the system keyring first; the backend reads them from there.
			if (SECRET[form.method] && form.token)
				await api.secretSet(profile, SECRET[form.method], form.token.trim());
			if (form.method === 'dnsRoute53') {
				if (form.accessKey)
					await api.secretSet(profile, 'nginx/dns/route53-access-key', form.accessKey.trim());
				if (form.secretKey)
					await api.secretSet(profile, 'nginx/dns/route53-secret-key', form.secretKey.trim());
			}
		} catch (error) {
			toast.error(error, 'Could not store the credentials');
			return;
		}
		const method = (
			form.method === 'webroot' ? { method: 'webroot', path: form.webroot } : { method: form.method }
		) as IssueMethod;
		issueOpen = false;
		await run(
			() => api.nginxCertIssue(target, { domains, email: form.email.trim(), method }),
			'Issue a certificate'
		);
	}

	export const refresh = () => certs.refresh();
</script>

<DependencyGuard tools={host ? ['certbot'] : []} active={visible}>
	<Page scroll={false}>
		{#snippet toolbar()}
			<p class="mr-auto text-xs text-muted-foreground">Let's Encrypt certificates managed by certbot.</p>
			<Button
				variant="outline"
				size="sm"
				onclick={() => act({ action: 'dryRun' }, 'Test certificate renewal')}>Dry run</Button
			>
			<Button
				variant="outline"
				size="sm"
				onclick={() => act({ action: 'renewAll' }, 'Renew all certificates')}>Renew all</Button
			>
			<Button size="sm" onclick={openIssue}><Plus /> Issue certificate</Button>
			<RefreshControl onrefresh={certs.refresh} loading={certs.loading} />
		{/snippet}
		<DataTable
			rows={certs.data ?? []}
			{columns}
			rowKey={(c) => c.name}
			loading={certs.loading}
			error={certs.error}
			onretry={certs.refresh}
			empty="No certificates"
			emptyHint="Issue one to enable HTTPS for a proxy host."
			class="flex-1"
		>
			{#snippet cell(cert, column)}
				{#if column.key === 'daysLeft'}
					{@const days = cert.daysLeft}
					<span
						class={cn(
							days !== null && days <= 3
								? 'font-semibold text-destructive'
								: days !== null && days <= 14
									? 'font-medium text-warning'
									: ''
						)}
					>
						{days === null ? '—' : days < 0 ? 'expired' : days}
					</span>
				{:else if column.key === 'name'}
					<span class="font-medium">{cert.name}</span>
				{:else}
					{column.value?.(cert)}
				{/if}
			{/snippet}
			{#snippet actions(cert)}
				<Button
					variant="ghost"
					size="xs"
					onclick={() => act({ action: 'renew', name: cert.name, force: false }, `Renew ${cert.name}`)}
					>Renew</Button
				>
				<Button
					variant="ghost"
					size="xs"
					onclick={() => act({ action: 'renew', name: cert.name, force: true }, `Force-renew ${cert.name}`)}
					>Force</Button
				>
				<Button variant="ghost" size="xs" onclick={() => remove(cert)}>Delete</Button>
			{/snippet}
		</DataTable>
	</Page>
</DependencyGuard>

<JobDialog bind:job />

<Modal bind:open={issueOpen} title="Issue certificate" size="lg">
	<form
		id="cert-form"
		class="flex flex-col gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void issue();
		}}
	>
		<Field
			label="Domain names"
			required
			hint="Separate several with commas. Wildcards (*.example.com) need a DNS challenge."
		>
			<Input
				bind:value={form.domains}
				placeholder="example.com, www.example.com"
				spellcheck="false"
				autofocus
			/>
		</Field>
		<Field label="Email" required hint="Let's Encrypt sends expiry warnings here."
			><Input type="email" bind:value={form.email} placeholder="admin@example.com" /></Field
		>
		<Field label="Validation method"><SelectField bind:value={form.method} options={METHODS} /></Field>
		{#if form.method === 'webroot'}
			<Field label="Webroot path"><PathInput bind:value={form.webroot} dirsOnly /></Field>
		{:else if SECRET[form.method]}
			<Field
				label="API token"
				hint={stored[SECRET[form.method]]
					? 'A token is stored in the keyring. Leave empty to use it.'
					: 'Stored in your system keyring; written to the server with mode 600.'}
			>
				<Input type="password" bind:value={form.token} autocomplete="off" />
			</Field>
		{:else if form.method === 'dnsRoute53'}
			<div class="grid grid-cols-2 gap-3">
				<Field
					label="AWS access key ID"
					hint={stored['nginx/dns/route53-access-key'] ? 'Stored. Leave empty to keep.' : undefined}
					><Input type="password" bind:value={form.accessKey} autocomplete="off" /></Field
				>
				<Field
					label="AWS secret access key"
					hint={stored['nginx/dns/route53-secret-key'] ? 'Stored. Leave empty to keep.' : undefined}
					><Input type="password" bind:value={form.secretKey} autocomplete="off" /></Field
				>
			</div>
		{/if}
		{#if host && PLUGIN[form.method]}
			{#key form.method}
				<DependencyGuard tools={PLUGIN[form.method]} inline>
					<span></span>
				</DependencyGuard>
			{/key}
		{/if}
		<p class="text-xs text-muted-foreground">
			A manual DNS challenge needs you to answer certbot's prompts; run <code>certbot certonly --manual</code> in
			the Terminal for that.
		</p>
	</form>
	{#snippet footer()}
		{#if issueError}<span class="mr-auto self-center text-xs text-muted-foreground">{issueError}</span>{/if}
		<Button variant="outline" onclick={() => (issueOpen = false)}>Cancel</Button>
		<Button type="submit" form="cert-form" disabled={issueError !== null}>Issue certificate</Button>
	{/snippet}
</Modal>
