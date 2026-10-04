<!-- Sites (tunnels), private site resources and public resources with their targets. -->
<script lang="ts">
	import { openUrl } from '@tauri-apps/plugin-opener';
	import Copy from '@lucide/svelte/icons/copy';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import X from '@lucide/svelte/icons/x';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { toIpcError, type IpcError } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { cn, copyText } from '$lib/utils';
	import CheckList from './CheckList.svelte';
	import { bool, list, listAll, orgPath, pangolin, pick, str, type Json } from './client';

	interface Props {
		visible: boolean;
	}

	let { visible }: Props = $props();

	let view = $state<'sites' | 'private' | 'public'>('sites');
	let search = $state('');
	let loading = $state(false);
	let error = $state<IpcError | null>(null);
	let sites = $state<Json[]>([]);
	let privates = $state<Json[]>([]);
	let publics = $state<Json[]>([]);
	let domains = $state<Json[]>([]);
	let users = $state<Json[]>([]);
	let roles = $state<Json[]>([]);
	let clients = $state<Json[]>([]);
	let saving = $state(false);

	async function load(): Promise<void> {
		loading = true;
		try {
			[sites, privates, publics] = await Promise.all([
				listAll(orgPath('/sites'), ['sites']),
				listAll(orgPath('/site-resources'), ['siteResources', 'resources']),
				listAll(orgPath('/resources'), ['resources'])
			]);
			error = null;
		} catch (raw) {
			error = toIpcError(raw);
		} finally {
			loading = false;
		}
	}

	/** Lookup lists for the forms; failures only leave a list empty. */
	async function loadChoices(): Promise<void> {
		const quiet = (p: Promise<Json[]>) => p.catch(() => [] as Json[]);
		[domains, users, roles, clients] = await Promise.all([
			quiet(listAll(orgPath('/domains'), ['domains'])),
			quiet(listAll(orgPath('/users'), ['users'])),
			quiet(listAll(orgPath('/roles'), ['roles'])),
			quiet(listAll(orgPath('/clients'), ['clients']))
		]);
	}

	let started = false;
	$effect(() => {
		if (!visible || started) return;
		started = true;
		void load();
		void loadChoices();
	});

	const siteOptions = $derived(sites.map((s) => ({ value: str(s.siteId), label: str(s.name) })));
	const siteName = (id: unknown) => siteOptions.find((s) => s.value === str(id))?.label ?? str(id);

	async function act(action: () => Promise<unknown>, done: string): Promise<boolean> {
		saving = true;
		try {
			await action();
			toast.success(done);
			await load();
			return true;
		} catch (e) {
			toast.error(e);
			return false;
		} finally {
			saving = false;
		}
	}

	async function remove(what: string, name: string, path: string, warning: string): Promise<void> {
		const ok = await confirm({
			title: `Delete ${what} “${name}”?`,
			message: warning,
			confirmLabel: 'Delete',
			destructive: true
		});
		if (ok) await act(() => pangolin('DELETE', path), `${name} deleted`);
	}

	// ------------------------------------------------------------ sites
	const siteColumns: Column<Json>[] = [
		{ key: 'name', label: 'Name', value: (s) => str(s.name), class: 'w-56 max-w-56' },
		{ key: 'niceId', label: 'Nice ID', value: (s) => str(s.niceId), mono: true, class: 'w-52 max-w-52' },
		{ key: 'type', label: 'Type', value: (s) => str(s.type), class: 'w-28' },
		{
			key: 'online',
			label: 'Status',
			value: (s) => (str(s.type) === 'local' ? 'Local' : bool(s.online) ? 'Online' : 'Offline'),
			class: 'w-24'
		},
		{ key: 'subnet', label: 'Subnet', value: (s) => str(s.subnet), mono: true, class: 'w-40' },
		{ key: 'address', label: 'Address', value: (s) => str(s.address), mono: true, class: 'w-40' },
		{
			key: 'pubKey',
			label: 'Public key',
			value: (s) => str(pick(s, 'pubKey', 'publicKey')),
			mono: true,
			class: 'max-w-0 w-full'
		}
	];

	let siteForm = $state<{ name: string; type: 'newt' | 'wireguard' | 'local'; pubKey: string } | null>(null);
	/** Credentials shown once after a site was created. */
	let created = $state<{ title: string; rows: [string, string][] } | null>(null);

	async function createSite(): Promise<void> {
		const form = siteForm;
		if (!form) return;
		saving = true;
		try {
			const body: Json = { name: form.name.trim(), type: form.type };
			let rows: [string, string][] = [];
			if (form.type !== 'local') {
				const defaults = await pangolin('GET', orgPath('/pick-site-defaults'));
				body.exitNodeId = defaults.exitNodeId;
				body.subnet = defaults.subnet;
				if (form.type === 'newt') {
					body.newtId = defaults.newtId;
					body.secret = defaults.newtSecret;
					body.address = defaults.clientAddress;
					rows = [
						['Newt ID', str(defaults.newtId)],
						['Newt secret', str(defaults.newtSecret)],
						['Endpoint', str(defaults.endpoint)]
					];
				} else {
					body.pubKey = form.pubKey.trim();
					rows = [
						['Server public key', str(defaults.publicKey)],
						['Endpoint', `${str(defaults.endpoint)}:${str(defaults.listenPort)}`],
						['Tunnel subnet', str(defaults.subnet)]
					];
				}
			}
			await pangolin('PUT', orgPath('/site'), { body });
			siteForm = null;
			toast.success('Site created');
			if (rows.length) created = { title: `Connect “${form.name.trim()}”`, rows: rows.filter(([, v]) => v) };
			await load();
		} catch (e) {
			toast.error(e, 'Could not create the site');
		} finally {
			saving = false;
		}
	}

	// ------------------------------------------------------------ private resources
	const privateColumns: Column<Json>[] = [
		{ key: 'name', label: 'Name', value: (r) => str(r.name), class: 'w-56 max-w-56' },
		{ key: 'mode', label: 'Mode', value: (r) => str(r.mode), class: 'w-20' },
		{
			key: 'destination',
			label: 'Destination',
			value: (r) => str(r.destination) + (r.destinationPort ? `:${str(r.destinationPort)}` : ''),
			mono: true,
			class: 'w-56 max-w-56'
		},
		{
			key: 'site',
			label: 'Site',
			value: (r) => str(pick(r, 'siteName')) || siteName(r.siteId),
			class: 'w-44 max-w-44'
		},
		{ key: 'tcp', label: 'TCP ports', value: (r) => str(r.tcpPortRangeString), mono: true, class: 'w-32' },
		{ key: 'udp', label: 'UDP ports', value: (r) => str(r.udpPortRangeString), mono: true, class: 'w-32' },
		{
			key: 'icmp',
			label: 'ICMP',
			value: (r) => (bool(r.disableIcmp) ? 'Off' : 'On'),
			class: 'max-w-0 w-full'
		}
	];

	interface PrivateForm {
		id: string;
		name: string;
		mode: 'host' | 'cidr' | 'http' | 'ssh';
		destination: string;
		destinationPort: string;
		alias: string;
		tcp: string;
		udp: string;
		icmp: boolean;
		siteIds: string[];
		userIds: string[];
		roleIds: string[];
		clientIds: string[];
	}
	let privateForm = $state<PrivateForm | null>(null);

	async function editPrivate(row: Json | null): Promise<void> {
		const id = row ? str(row.siteResourceId) : '';
		const form: PrivateForm = {
			id,
			name: str(row?.name),
			mode: (str(row?.mode) || 'host') as PrivateForm['mode'],
			destination: str(row?.destination),
			destinationPort: str(row?.destinationPort),
			alias: str(row?.alias),
			tcp: str(row?.tcpPortRangeString) || '*',
			udp: str(row?.udpPortRangeString) || '*',
			icmp: !bool(row?.disableIcmp),
			siteIds: row
				? list(row, 'siteIds').length
					? (row.siteIds as unknown[]).map(str)
					: [str(row.siteId)].filter(Boolean)
				: [],
			userIds: [],
			roleIds: [],
			clientIds: []
		};
		privateForm = form;
		if (!id) return;
		// Who may reach it is stored separately.
		const ids = async (suffix: string, keys: string[], field: string) =>
			list(await pangolin('GET', `/v1/site-resource/${id}/${suffix}`).catch(() => ({})), ...keys).map((x) =>
				str(pick(x, field, 'id'))
			);
		const [u, r, c] = await Promise.all([
			ids('users', ['users'], 'userId'),
			ids('roles', ['roles'], 'roleId'),
			ids('clients', ['clients'], 'clientId')
		]);
		if (privateForm?.id === id) Object.assign(privateForm, { userIds: u, roleIds: r, clientIds: c });
	}

	async function savePrivate(): Promise<void> {
		const f = privateForm;
		if (!f) return;
		const body: Json = {
			name: f.name.trim(),
			mode: f.mode,
			destination: f.destination.trim(),
			siteIds: f.siteIds.map(Number),
			userIds: f.userIds,
			roleIds: f.roleIds.map(Number),
			clientIds: f.clientIds.map(Number),
			tcpPortRangeString: f.tcp.trim(),
			udpPortRangeString: f.udp.trim(),
			disableIcmp: !f.icmp
		};
		if (f.alias.trim()) body.alias = f.alias.trim();
		if (f.destinationPort.trim()) body.destinationPort = Number(f.destinationPort);
		const ok = await act(
			() =>
				f.id
					? pangolin('POST', `/v1/site-resource/${f.id}`, { body })
					: pangolin('PUT', orgPath('/site-resource'), { body }),
			f.id ? 'Resource updated' : 'Resource created'
		);
		if (ok) privateForm = null;
	}

	// ------------------------------------------------------------ public resources
	const fullDomain = (r: Json) => str(pick(r, 'fullDomain'));
	const publicMode = (r: Json) => str(pick(r, 'mode')) || (bool(r.http) ? 'http' : str(r.protocol));
	const publicColumns: Column<Json>[] = [
		{ key: 'name', label: 'Name', value: (r) => str(r.name), class: 'w-56 max-w-56' },
		{ key: 'mode', label: 'Mode', value: publicMode, class: 'w-20' },
		{
			key: 'access',
			label: 'Address',
			value: (r) => fullDomain(r) || (r.proxyPort ? `port ${str(r.proxyPort)}` : ''),
			class: 'max-w-0 w-full'
		},
		{ key: 'sticky', label: 'Sticky', value: (r) => (bool(r.stickySession) ? 'Yes' : 'No'), class: 'w-20' },
		{
			key: 'enabled',
			label: 'Enabled',
			value: (r) => (r.enabled === undefined || bool(r.enabled) ? 'Yes' : 'No'),
			class: 'w-24'
		}
	];

	interface Target {
		id: string;
		siteId: string;
		ip: string;
		port: string;
		method: string;
		/** Removed in the form; deleted on save. */
		removed: boolean;
	}
	interface PublicForm {
		id: string;
		name: string;
		mode: 'http' | 'ssh' | 'rdp' | 'vnc' | 'tcp' | 'udp';
		domainId: string;
		subdomain: string;
		proxyPort: string;
		sticky: boolean;
		postAuthPath: string;
		enabled: boolean;
		targets: Target[];
	}
	let publicForm = $state<PublicForm | null>(null);
	const raw = (mode: string) => mode === 'tcp' || mode === 'udp';

	async function editPublic(row: Json | null): Promise<void> {
		const id = row ? str(row.resourceId) : '';
		publicForm = {
			id,
			name: str(row?.name),
			mode: (row ? publicMode(row) || 'http' : 'http') as PublicForm['mode'],
			domainId: str(row?.domainId) || str(domains[0]?.domainId),
			subdomain: str(row?.subdomain),
			proxyPort: str(row?.proxyPort),
			sticky: bool(row?.stickySession),
			postAuthPath: str(row?.postAuthPath),
			enabled: row ? row.enabled === undefined || bool(row.enabled) : true,
			targets: []
		};
		if (!id) return;
		const found = list(await pangolin('GET', `/v1/resource/${id}/targets`).catch(() => ({})), 'targets');
		if (publicForm?.id === id) {
			publicForm.targets = found.map((t) => ({
				id: str(t.targetId),
				siteId: str(t.siteId),
				ip: str(t.ip),
				port: str(t.port),
				method: str(t.method) || 'http',
				removed: false
			}));
		}
	}

	async function savePublic(): Promise<void> {
		const f = publicForm;
		if (!f) return;
		const isRaw = raw(f.mode);
		saving = true;
		try {
			let id = f.id;
			if (!id) {
				const body: Json = isRaw
					? {
							name: f.name.trim(),
							http: false,
							protocol: f.mode,
							mode: f.mode,
							proxyPort: Number(f.proxyPort)
						}
					: {
							name: f.name.trim(),
							http: true,
							protocol: 'tcp',
							mode: f.mode,
							domainId: f.domainId,
							subdomain: f.subdomain.trim() || undefined,
							stickySession: f.sticky,
							postAuthPath: f.postAuthPath.trim() || undefined
						};
				const createdRow = await pangolin('PUT', orgPath('/resource'), { body });
				id = str(pick(createdRow, 'resourceId'));
			} else {
				const body: Json = isRaw
					? {
							name: f.name.trim(),
							proxyPort: Number(f.proxyPort),
							stickySession: f.sticky,
							enabled: f.enabled
						}
					: {
							name: f.name.trim(),
							domainId: f.domainId,
							subdomain: f.subdomain.trim() || null,
							stickySession: f.sticky,
							postAuthPath: f.postAuthPath.trim() || null,
							enabled: f.enabled
						};
				await pangolin('POST', `/v1/resource/${id}`, { body });
			}
			for (const t of f.targets) {
				if (t.removed) {
					if (t.id) await pangolin('DELETE', `/v1/target/${t.id}`);
					continue;
				}
				const body: Json = { siteId: Number(t.siteId), ip: t.ip.trim(), port: Number(t.port) };
				if (!isRaw) body.method = t.method;
				if (t.id) await pangolin('POST', `/v1/target/${t.id}`, { body });
				else if (id) await pangolin('PUT', `/v1/resource/${id}/target`, { body });
			}
			toast.success(f.id ? 'Resource updated' : 'Resource created');
			publicForm = null;
			await load();
		} catch (e) {
			toast.error(e, 'Could not save the resource');
			await load();
		} finally {
			saving = false;
		}
	}

	const publicError = $derived.by(() => {
		const f = publicForm;
		if (!f) return null;
		if (!f.name.trim()) return 'Enter a name.';
		if (raw(f.mode) ? !(Number(f.proxyPort) > 0 && Number(f.proxyPort) < 65536) : !f.domainId)
			return raw(f.mode) ? 'Enter the proxy port.' : 'Choose a domain.';
		for (const t of f.targets.filter((t) => !t.removed)) {
			if (!t.siteId || !t.ip.trim() || !(Number(t.port) > 0 && Number(t.port) < 65536))
				return 'Every target needs a site, an address and a port.';
		}
		return null;
	});

	export const refresh = load;
</script>

<div class="flex min-h-0 flex-1 flex-col gap-2">
	<div class="flex flex-wrap items-center gap-2">
		<SubTabs
			bind:value={view}
			items={[
				{ id: 'sites', label: 'Sites', count: sites.length },
				{ id: 'private', label: 'Private resources', count: privates.length },
				{ id: 'public', label: 'Public resources', count: publics.length }
			]}
		/>
		<span class="flex-1"></span>
		<SearchInput bind:value={search} class="w-56" />
		{#if view === 'sites'}
			<Button size="sm" onclick={() => (siteForm = { name: '', type: 'newt', pubKey: '' })}
				><Plus /> New site</Button
			>
		{:else if view === 'private'}
			<Button size="sm" onclick={() => editPrivate(null)}><Plus /> New private resource</Button>
		{:else}
			<Button size="sm" onclick={() => editPublic(null)}><Plus /> New public resource</Button>
		{/if}
		<RefreshControl onrefresh={load} {loading} />
	</div>

	{#if view === 'sites'}
		<DataTable
			rows={sites}
			columns={siteColumns}
			rowKey={(s) => str(s.siteId)}
			{search}
			{loading}
			{error}
			onretry={load}
			empty="No sites yet"
			emptyHint="A site is a tunnel from Pangolin to one of your networks."
			class="flex-1"
		>
			{#snippet cell(s, column)}
				{#if column.key === 'online'}
					{@const label = String(column.value?.(s))}
					<span
						class={cn(
							'rounded px-1.5 py-0.5 text-[11px] font-medium',
							label === 'Online'
								? 'bg-success/15 text-success'
								: label === 'Offline'
									? 'bg-destructive/15 text-destructive'
									: 'bg-muted'
						)}>{label}</span
					>
				{:else}<span title={String(column.value?.(s) ?? '')}>{column.value?.(s)}</span>{/if}
			{/snippet}
			{#snippet actions(s)}
				<IconButton
					label="Delete site"
					onclick={() =>
						remove(
							'site',
							str(s.name),
							`/v1/site/${str(s.siteId)}`,
							'The tunnel stops working and targets that use this site lose their route.'
						)}><Trash2 /></IconButton
				>
			{/snippet}
		</DataTable>
	{:else if view === 'private'}
		<DataTable
			rows={privates}
			columns={privateColumns}
			rowKey={(r) => str(r.siteResourceId)}
			{search}
			{loading}
			{error}
			onretry={load}
			empty="No private resources yet"
			emptyHint="Private resources are reached through the Pangolin client, not from the internet."
			onrowdblclick={editPrivate}
			class="flex-1"
		>
			{#snippet actions(r)}
				<IconButton label="Edit" onclick={() => editPrivate(r)}><Pencil /></IconButton>
				<IconButton
					label="Delete"
					onclick={() =>
						remove(
							'private resource',
							str(r.name),
							`/v1/site-resource/${str(r.siteResourceId)}`,
							'Clients lose access to it immediately.'
						)}><Trash2 /></IconButton
				>
			{/snippet}
		</DataTable>
	{:else}
		<DataTable
			rows={publics}
			columns={publicColumns}
			rowKey={(r) => str(r.resourceId)}
			{search}
			{loading}
			{error}
			onretry={load}
			empty="No public resources yet"
			emptyHint="Public resources are published on a domain or a port."
			onrowdblclick={editPublic}
			class="flex-1"
		>
			{#snippet cell(r, column)}
				{#if column.key === 'access' && fullDomain(r)}
					<button
						type="button"
						class="inline-flex items-center gap-1 text-primary hover:underline"
						onclick={() =>
							openUrl(`${bool(r.ssl) || r.ssl === undefined ? 'https' : 'http'}://${fullDomain(r)}`)}
					>
						{fullDomain(r)}
						<ExternalLink class="size-3" />
					</button>
				{:else}{column.value?.(r)}{/if}
			{/snippet}
			{#snippet actions(r)}
				<IconButton label="Edit" onclick={() => editPublic(r)}><Pencil /></IconButton>
				<IconButton
					label="Delete"
					onclick={() =>
						remove(
							'public resource',
							str(r.name),
							`/v1/resource/${str(r.resourceId)}`,
							'It stops being reachable immediately and its targets are removed.'
						)}><Trash2 /></IconButton
				>
			{/snippet}
		</DataTable>
	{/if}
</div>

{#if siteForm}
	{@const form = siteForm}
	<Modal open title="New site" size="md" onclose={() => (siteForm = null)}>
		<div class="flex flex-col gap-3">
			<Field label="Name"><Input bind:value={form.name} autofocus /></Field>
			<Field label="Type">
				<SelectField
					bind:value={form.type}
					options={[
						{ value: 'newt', label: 'Newt (recommended)' },
						{ value: 'wireguard', label: 'Basic WireGuard' },
						{ value: 'local', label: 'Local (no tunnel)' }
					]}
				/>
			</Field>
			{#if form.type === 'wireguard'}
				<Field
					label="Your WireGuard public key"
					hint="Generate a key pair on the site (wg genkey | tee private.key | wg pubkey) and paste the public key."
				>
					<Input bind:value={form.pubKey} class="font-mono text-xs" spellcheck="false" />
				</Field>
			{:else if form.type === 'newt'}
				<p class="text-xs text-muted-foreground">
					Pangolin generates the Newt ID and secret. They are shown once after the site is created.
				</p>
			{/if}
		</div>
		{#snippet footer()}
			<Button variant="outline" onclick={() => (siteForm = null)}>Cancel</Button>
			<Button
				disabled={saving ||
					!form.name.trim() ||
					(form.type === 'wireguard' && form.pubKey.trim().length < 40)}
				onclick={createSite}>{saving ? 'Creating…' : 'Create site'}</Button
			>
		{/snippet}
	</Modal>
{/if}

{#if created}
	{@const info = created}
	<Modal
		open
		title={info.title}
		description="Copy these values now: the secret is not shown again."
		size="lg"
		dismissible={false}
		onclose={() => (created = null)}
	>
		<dl class="grid grid-cols-[10rem_1fr_auto] items-center gap-x-3 gap-y-2 text-sm">
			{#each info.rows as [label, value] (label)}
				<dt class="text-muted-foreground">{label}</dt>
				<dd class="selectable font-mono text-xs break-all">{value}</dd>
				<IconButton label="Copy {label}" onclick={() => copyText(value).then(() => toast.success('Copied'))}
					><Copy /></IconButton
				>
			{/each}
		</dl>
		{#snippet footer()}
			<Button onclick={() => (created = null)}>I have saved them</Button>
		{/snippet}
	</Modal>
{/if}

{#if privateForm}
	{@const form = privateForm}
	<Modal
		open
		title={form.id ? 'Edit private resource' : 'New private resource'}
		size="xl"
		onclose={() => (privateForm = null)}
	>
		<div class="grid grid-cols-2 gap-3">
			<Field label="Name"><Input bind:value={form.name} autofocus /></Field>
			<Field label="Mode">
				<SelectField
					bind:value={form.mode}
					options={[
						{ value: 'host', label: 'Host (one address)' },
						{ value: 'cidr', label: 'CIDR (a network range)' },
						{ value: 'http', label: 'HTTP' },
						{ value: 'ssh', label: 'SSH' }
					]}
				/>
			</Field>
			<Field
				label="Destination"
				hint={form.mode === 'cidr' ? 'e.g. 10.0.0.0/24' : 'IP address or host name on the site’s network'}
			>
				<Input bind:value={form.destination} class="font-mono text-xs" spellcheck="false" />
			</Field>
			{#if form.mode === 'http' || form.mode === 'ssh'}
				<Field label="Destination port"
					><Input type="number" min="1" max="65535" bind:value={form.destinationPort} /></Field
				>
			{:else if form.mode === 'host'}
				<Field label="Alias" hint="Optional DNS name clients can use."
					><Input bind:value={form.alias} placeholder="db.internal" spellcheck="false" /></Field
				>
			{:else}
				<span></span>
			{/if}
			<Field label="TCP ports" hint="* for all, empty for none, or e.g. 80,443,8000-9000"
				><Input bind:value={form.tcp} class="font-mono text-xs" /></Field
			>
			<Field label="UDP ports"><Input bind:value={form.udp} class="font-mono text-xs" /></Field>
			<label class="col-span-2 flex items-center gap-2 text-sm"
				><Checkbox bind:checked={form.icmp} /> Allow ICMP (ping)</label
			>
			<Field label="Sites"
				><CheckList options={siteOptions} bind:selected={form.siteIds} empty="No sites" /></Field
			>
			<Field label="Roles"
				><CheckList
					options={roles.map((r) => ({ value: str(r.roleId), label: str(r.name) }))}
					bind:selected={form.roleIds}
					empty="No roles"
				/></Field
			>
			<Field label="Users"
				><CheckList
					options={users.map((u) => ({
						value: str(pick(u, 'id', 'userId')),
						label: str(pick(u, 'email', 'username', 'name'))
					}))}
					bind:selected={form.userIds}
					empty="No users"
				/></Field
			>
			<Field label="Machine clients"
				><CheckList
					options={clients.map((c) => ({ value: str(c.clientId), label: str(c.name) }))}
					bind:selected={form.clientIds}
					empty="No clients"
				/></Field
			>
		</div>
		{#snippet footer()}
			{#if form.siteIds.length === 0}<p class="mr-auto text-xs text-muted-foreground">
					Choose at least one site.
				</p>{/if}
			<Button variant="outline" onclick={() => (privateForm = null)}>Cancel</Button>
			<Button
				disabled={saving || !form.name.trim() || !form.destination.trim() || form.siteIds.length === 0}
				onclick={savePrivate}>{saving ? 'Saving…' : 'Save'}</Button
			>
		{/snippet}
	</Modal>
{/if}

{#if publicForm}
	{@const form = publicForm}
	{@const isRaw = raw(form.mode)}
	<Modal
		open
		title={form.id ? 'Edit public resource' : 'New public resource'}
		size="xl"
		onclose={() => (publicForm = null)}
	>
		<div class="grid grid-cols-2 gap-3">
			<Field label="Name"><Input bind:value={form.name} autofocus /></Field>
			<Field label="Mode" hint={form.id ? 'The mode cannot be changed afterwards.' : undefined}>
				<SelectField
					bind:value={form.mode}
					disabled={!!form.id}
					options={[
						{ value: 'http', label: 'HTTP / HTTPS' },
						{ value: 'ssh', label: 'SSH' },
						{ value: 'rdp', label: 'RDP' },
						{ value: 'vnc', label: 'VNC' },
						{ value: 'tcp', label: 'Raw TCP' },
						{ value: 'udp', label: 'Raw UDP' }
					]}
				/>
			</Field>
			{#if isRaw}
				<Field label="Proxy port" hint="The port Pangolin listens on."
					><Input type="number" min="1" max="65535" bind:value={form.proxyPort} /></Field
				>
				<span></span>
			{:else}
				<Field label="Subdomain" hint="Leave empty to use the domain itself."
					><Input bind:value={form.subdomain} placeholder="app" spellcheck="false" /></Field
				>
				<Field label="Domain">
					<SelectField
						bind:value={form.domainId}
						options={domains.map((d) => ({ value: str(d.domainId), label: str(d.baseDomain) }))}
						placeholder={domains.length ? 'Choose a domain…' : 'No domains configured'}
					/>
				</Field>
				<Field
					label="Post-authentication path"
					class="col-span-2"
					hint="Optional: where users land after signing in, e.g. /dashboard"
					><Input bind:value={form.postAuthPath} spellcheck="false" /></Field
				>
			{/if}
			<label class="flex items-center gap-2 text-sm"
				><Checkbox bind:checked={form.sticky} /> Sticky sessions</label
			>
			{#if form.id}<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={form.enabled} /> Enabled</label
				>{/if}

			<div class="col-span-2">
				<div class="mb-1 flex items-center gap-2">
					<h3 class="text-sm font-semibold">Targets</h3>
					<Button
						variant="ghost"
						size="xs"
						onclick={() =>
							form.targets.push({
								id: '',
								siteId: siteOptions[0]?.value ?? '',
								ip: '',
								port: '',
								method: 'http',
								removed: false
							})}><Plus /> Add target</Button
					>
				</div>
				{#each form.targets as target, i (i)}
					{#if !target.removed}
						<div class="mb-1.5 flex items-center gap-2">
							<SelectField bind:value={target.siteId} options={siteOptions} placeholder="Site" class="w-48" />
							{#if !isRaw}<SelectField
									bind:value={target.method}
									options={['http', 'https', 'h2c']}
									class="w-24"
								/>{/if}
							<Input
								bind:value={target.ip}
								placeholder="IP or host name"
								class="flex-1 font-mono text-xs"
								spellcheck="false"
							/>
							<Input
								type="number"
								min="1"
								max="65535"
								bind:value={target.port}
								placeholder="Port"
								class="w-24"
							/>
							<IconButton
								label="Remove target"
								onclick={() => (target.id ? (target.removed = true) : form.targets.splice(i, 1))}
								><X /></IconButton
							>
						</div>
					{/if}
				{:else}
					<p class="text-xs text-muted-foreground">
						No targets: the resource has nowhere to send requests yet.
					</p>
				{/each}
			</div>
		</div>
		{#snippet footer()}
			{#if publicError}<p class="mr-auto text-xs text-muted-foreground">{publicError}</p>{/if}
			<Button variant="outline" onclick={() => (publicForm = null)}>Cancel</Button>
			<Button disabled={saving || !!publicError} onclick={savePublic}>{saving ? 'Saving…' : 'Save'}</Button>
		{/snippet}
	</Modal>
{/if}
