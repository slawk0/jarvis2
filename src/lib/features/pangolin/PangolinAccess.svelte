<!-- Users (read-only), roles (CRUD), identity providers (read-only) and invitations. -->
<script lang="ts">
	import Copy from '@lucide/svelte/icons/copy';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
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
	import { Textarea } from '$lib/components/ui/textarea';
	import { formatDateTime } from '$lib/format';
	import { toIpcError, type IpcError } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { copyText } from '$lib/utils';
	import CheckList from './CheckList.svelte';
	import { bool, list, listAll, orgPath, pangolin, pick, str, toDate, type Json } from './client';

	interface Props {
		visible: boolean;
	}

	let { visible }: Props = $props();

	let view = $state<'users' | 'roles' | 'idps' | 'invitations'>('users');
	let search = $state('');
	let loading = $state(false);
	let error = $state<IpcError | null>(null);
	let users = $state<Json[]>([]);
	let roles = $state<Json[]>([]);
	let idps = $state<Json[]>([]);
	let invitations = $state<Json[]>([]);
	let saving = $state(false);

	async function load(): Promise<void> {
		loading = true;
		try {
			// Identity providers and invitations may be outside the key's permissions.
			const optional = (p: Promise<Json[]>) => p.catch(() => [] as Json[]);
			[users, roles, idps, invitations] = await Promise.all([
				listAll(orgPath('/users'), ['users']),
				listAll(orgPath('/roles'), ['roles']),
				optional(listAll(orgPath('/idp'), ['idps', 'idp'])),
				optional(listAll(orgPath('/invitations'), ['invitations']))
			]);
			error = null;
		} catch (raw) {
			error = toIpcError(raw);
		} finally {
			loading = false;
		}
	}

	let started = false;
	$effect(() => {
		if (!visible || started) return;
		started = true;
		void load();
	});

	const roleNames = (row: Json): string => {
		const many = list(row, 'roles').map((r) => str(pick(r, 'roleName', 'name')));
		return many.length ? many.join(', ') : str(pick(row, 'roleName', 'role'));
	};
	const lines = (text: string) =>
		text
			.split(/[\n,]/)
			.map((l) => l.trim())
			.filter(Boolean);

	const userColumns: Column<Json>[] = [
		{ key: 'email', label: 'Email', value: (u) => str(pick(u, 'email', 'username')), class: 'w-72 max-w-72' },
		{ key: 'name', label: 'Name', value: (u) => str(u.name), class: 'w-56 max-w-56' },
		{
			key: 'twoFactor',
			label: '2FA',
			value: (u) => (bool(pick(u, 'twoFactorEnabled', 'twoFactor')) ? 'On' : 'Off'),
			class: 'w-20'
		},
		{
			key: 'idp',
			label: 'Sign-in',
			value: (u) => str(pick(u, 'idpName', 'type')) || 'Internal',
			class: 'w-40'
		},
		{
			key: 'roles',
			label: 'Roles',
			value: (u) => (bool(u.isOwner) ? 'Owner' : roleNames(u)),
			class: 'max-w-0 w-full'
		}
	];
	const roleColumns: Column<Json>[] = [
		{ key: 'name', label: 'Name', value: (r) => str(r.name), class: 'w-56 max-w-56' },
		{ key: 'description', label: 'Description', value: (r) => str(r.description), class: 'max-w-0 w-full' },
		{
			key: 'approval',
			label: 'Device approval',
			value: (r) => (bool(r.requireDeviceApproval) ? 'Required' : 'No'),
			class: 'w-36'
		},
		{
			key: 'ssh',
			label: 'SSH',
			value: (r) => (bool(r.allowSsh) ? `Allowed · sudo ${str(r.sshSudoMode) || 'none'}` : 'No'),
			class: 'w-48'
		}
	];
	const idpColumns: Column<Json>[] = [
		{ key: 'name', label: 'Name', value: (i) => str(i.name), class: 'w-72 max-w-72' },
		{ key: 'type', label: 'Type', value: (i) => str(pick(i, 'type', 'variant')), class: 'w-40' },
		{
			key: 'auto',
			label: 'Auto-provision',
			value: (i) => (bool(i.autoProvision) ? 'Yes' : 'No'),
			class: 'max-w-0 w-full'
		}
	];
	const inviteColumns: Column<Json>[] = [
		{ key: 'email', label: 'Email', value: (i) => str(i.email), class: 'w-80 max-w-80' },
		{ key: 'roles', label: 'Roles', value: roleNames, class: 'w-64 max-w-64' },
		{
			key: 'expires',
			label: 'Expires',
			value: (i) => toDate(i.expiresAt)?.getTime() ?? 0,
			class: 'max-w-0 w-full tabular'
		}
	];

	// ------------------------------------------------------------ roles
	interface RoleForm {
		id: string;
		name: string;
		description: string;
		requireDeviceApproval: boolean;
		allowSsh: boolean;
		sshSudoMode: 'none' | 'full' | 'commands';
		sshSudoCommands: string;
		sshCreateHomeDir: boolean;
		sshUnixGroups: string;
	}
	let roleForm = $state<RoleForm | null>(null);

	const asList = (v: unknown): string[] => {
		if (Array.isArray(v)) return v.map(str);
		if (typeof v === 'string' && v.trim().startsWith('[')) {
			try {
				return (JSON.parse(v) as unknown[]).map(str);
			} catch {
				return [];
			}
		}
		return typeof v === 'string' && v ? lines(v) : [];
	};

	function editRole(row: Json | null): void {
		roleForm = {
			id: row ? str(row.roleId) : '',
			name: str(row?.name),
			description: str(row?.description),
			requireDeviceApproval: bool(row?.requireDeviceApproval),
			allowSsh: bool(row?.allowSsh),
			sshSudoMode: (str(row?.sshSudoMode) || 'none') as RoleForm['sshSudoMode'],
			sshSudoCommands: asList(row?.sshSudoCommands).join('\n'),
			sshCreateHomeDir: bool(row?.sshCreateHomeDir),
			sshUnixGroups: asList(row?.sshUnixGroups).join(', ')
		};
	}

	async function saveRole(): Promise<void> {
		const f = roleForm;
		if (!f) return;
		saving = true;
		try {
			const body: Json = {
				name: f.name.trim(),
				description: f.description.trim(),
				requireDeviceApproval: f.requireDeviceApproval,
				allowSsh: f.allowSsh,
				sshSudoMode: f.sshSudoMode,
				sshSudoCommands: f.sshSudoMode === 'commands' ? lines(f.sshSudoCommands) : [],
				sshCreateHomeDir: f.sshCreateHomeDir,
				sshUnixGroups: lines(f.sshUnixGroups)
			};
			if (f.id) await pangolin('POST', `/v1/role/${f.id}`, { body });
			else await pangolin('PUT', orgPath('/role'), { body });
			toast.success(f.id ? 'Role updated' : 'Role created');
			roleForm = null;
			await load();
		} catch (e) {
			toast.error(e, 'Could not save the role');
		} finally {
			saving = false;
		}
	}

	/** Deleting a role needs another role for its members to move to. */
	let deleting = $state<{ role: Json; moveTo: string } | null>(null);

	async function deleteRole(): Promise<void> {
		const d = deleting;
		if (!d) return;
		saving = true;
		try {
			await pangolin('DELETE', `/v1/role/${str(d.role.roleId)}`, { body: { roleId: Number(d.moveTo) } });
			toast.success('Role deleted');
			deleting = null;
			await load();
		} catch (e) {
			toast.error(e, 'Could not delete the role');
		} finally {
			saving = false;
		}
	}

	// ------------------------------------------------------------ invitations
	let invite = $state<{ email: string; roleIds: string[]; validHours: string; sendEmail: boolean } | null>(
		null
	);
	let inviteLink = $state('');

	async function sendInvite(): Promise<void> {
		const f = invite;
		if (!f) return;
		saving = true;
		try {
			const data = await pangolin('POST', orgPath('/create-invite'), {
				body: {
					email: f.email.trim(),
					roleIds: f.roleIds.map(Number),
					validHours: Number(f.validHours),
					sendEmail: f.sendEmail
				}
			});
			invite = null;
			inviteLink = str(pick(data, 'inviteLink', 'link'));
			toast.success('Invitation created');
			await load();
		} catch (e) {
			toast.error(e, 'Could not create the invitation');
		} finally {
			saving = false;
		}
	}

	async function cancelInvite(row: Json): Promise<void> {
		const ok = await confirm({
			title: `Cancel the invitation for ${str(row.email)}?`,
			message: 'The invitation link stops working.',
			confirmLabel: 'Cancel invitation',
			destructive: true
		});
		if (!ok) return;
		try {
			await pangolin('DELETE', orgPath(`/invitations/${str(row.inviteId)}`));
			toast.success('Invitation cancelled');
			await load();
		} catch (e) {
			toast.error(e);
		}
	}

	export const refresh = load;
</script>

<div class="flex min-h-0 flex-1 flex-col gap-2">
	<div class="flex flex-wrap items-center gap-2">
		<SubTabs
			bind:value={view}
			items={[
				{ id: 'users', label: 'Users', count: users.length },
				{ id: 'roles', label: 'Roles', count: roles.length },
				{ id: 'idps', label: 'Identity providers', count: idps.length },
				{ id: 'invitations', label: 'Invitations', count: invitations.length }
			]}
		/>
		<span class="flex-1"></span>
		<SearchInput bind:value={search} class="w-56" />
		{#if view === 'roles'}
			<Button size="sm" onclick={() => editRole(null)}><Plus /> New role</Button>
		{:else if view === 'invitations'}
			<Button
				size="sm"
				onclick={() => (invite = { email: '', roleIds: [], validHours: '72', sendEmail: true })}
				><Plus /> Invite user</Button
			>
		{/if}
		<RefreshControl onrefresh={load} {loading} />
	</div>

	{#if view === 'users'}
		<DataTable
			rows={users}
			columns={userColumns}
			rowKey={(u) => str(pick(u, 'id', 'userId', 'email'))}
			{search}
			{loading}
			{error}
			onretry={load}
			empty="No users"
			class="flex-1"
		/>
	{:else if view === 'roles'}
		<DataTable
			rows={roles}
			columns={roleColumns}
			rowKey={(r) => str(r.roleId)}
			{search}
			{loading}
			{error}
			onretry={load}
			empty="No roles"
			onrowdblclick={(r) => !bool(r.isAdmin) && editRole(r)}
			class="flex-1"
		>
			{#snippet actions(r)}
				{#if bool(r.isAdmin)}
					<span class="px-2 text-xs text-muted-foreground">Built-in</span>
				{:else}
					<IconButton label="Edit role" onclick={() => editRole(r)}><Pencil /></IconButton>
					<IconButton
						label="Delete role"
						onclick={() =>
							(deleting = {
								role: r,
								moveTo: str(
									roles.find((x) => x.roleId !== r.roleId && !bool(x.isAdmin))?.roleId ??
										roles.find((x) => x.roleId !== r.roleId)?.roleId
								)
							})}><Trash2 /></IconButton
					>
				{/if}
			{/snippet}
		</DataTable>
	{:else if view === 'idps'}
		<DataTable
			rows={idps}
			columns={idpColumns}
			rowKey={(i) => str(i.idpId)}
			{search}
			{loading}
			{error}
			onretry={load}
			empty="No identity providers"
			emptyHint="Identity providers are managed in Pangolin itself."
			class="flex-1"
		/>
	{:else}
		<DataTable
			rows={invitations}
			columns={inviteColumns}
			rowKey={(i) => str(i.inviteId)}
			{search}
			{loading}
			{error}
			onretry={load}
			empty="No open invitations"
			class="flex-1"
		>
			{#snippet cell(i, column)}
				{#if column.key === 'expires'}{@const date = toDate(i.expiresAt)}{date
						? formatDateTime(date)
						: ''}{:else}{column.value?.(i)}{/if}
			{/snippet}
			{#snippet actions(i)}
				<Button variant="ghost" size="xs" onclick={() => cancelInvite(i)}>Cancel</Button>
			{/snippet}
		</DataTable>
	{/if}
</div>

{#if roleForm}
	{@const form = roleForm}
	<Modal open title={form.id ? 'Edit role' : 'New role'} size="lg" onclose={() => (roleForm = null)}>
		<div class="flex flex-col gap-3">
			<Field label="Name"><Input bind:value={form.name} autofocus /></Field>
			<Field label="Description"><Input bind:value={form.description} /></Field>
			<label class="flex items-center gap-2 text-sm"
				><Checkbox bind:checked={form.requireDeviceApproval} /> Require device approval</label
			>
			<label class="flex items-center gap-2 text-sm"
				><Checkbox bind:checked={form.allowSsh} /> Allow SSH</label
			>
			{#if form.allowSsh}
				<Field label="sudo">
					<SelectField
						bind:value={form.sshSudoMode}
						options={[
							{ value: 'none', label: 'No sudo' },
							{ value: 'full', label: 'Full sudo' },
							{ value: 'commands', label: 'Only specific commands' }
						]}
					/>
				</Field>
				{#if form.sshSudoMode === 'commands'}
					<Field label="Allowed sudo commands" hint="One per line.">
						<Textarea
							bind:value={form.sshSudoCommands}
							rows={3}
							class="font-mono text-xs"
							spellcheck="false"
						/>
					</Field>
				{/if}
				<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={form.sshCreateHomeDir} /> Create a home directory</label
				>
				<Field label="Unix groups" hint="Separated by commas."
					><Input bind:value={form.sshUnixGroups} placeholder="docker, www-data" spellcheck="false" /></Field
				>
			{/if}
		</div>
		{#snippet footer()}
			<Button variant="outline" onclick={() => (roleForm = null)}>Cancel</Button>
			<Button disabled={saving || !form.name.trim()} onclick={saveRole}>{saving ? 'Saving…' : 'Save'}</Button>
		{/snippet}
	</Modal>
{/if}

{#if deleting}
	{@const d = deleting}
	<Modal
		open
		title="Delete the role “{str(d.role.name)}”?"
		description="Its members are moved to another role. This cannot be undone."
		size="md"
		onclose={() => (deleting = null)}
	>
		<Field label="Move members to">
			<SelectField
				bind:value={d.moveTo}
				options={roles
					.filter((r) => r.roleId !== d.role.roleId)
					.map((r) => ({ value: str(r.roleId), label: str(r.name) }))}
				placeholder="Choose a role…"
			/>
		</Field>
		{#snippet footer()}
			<Button variant="outline" onclick={() => (deleting = null)}>Cancel</Button>
			<Button variant="destructive" disabled={saving || !d.moveTo} onclick={deleteRole}>Delete role</Button>
		{/snippet}
	</Modal>
{/if}

{#if invite}
	{@const form = invite}
	<Modal open title="Invite a user" size="md" onclose={() => (invite = null)}>
		<div class="flex flex-col gap-3">
			<Field label="Email"><Input type="email" bind:value={form.email} autofocus spellcheck="false" /></Field>
			<Field label="Roles"
				><CheckList
					options={roles.map((r) => ({ value: str(r.roleId), label: str(r.name) }))}
					bind:selected={form.roleIds}
					empty="No roles"
				/></Field
			>
			<Field label="Valid for">
				<SelectField
					bind:value={form.validHours}
					options={[
						{ value: '24', label: '1 day' },
						{ value: '72', label: '3 days' },
						{ value: '168', label: '7 days' },
						{ value: '720', label: '30 days' }
					]}
				/>
			</Field>
			<label class="flex items-center gap-2 text-sm"
				><Checkbox bind:checked={form.sendEmail} /> Send the invitation by email</label
			>
		</div>
		{#snippet footer()}
			<Button variant="outline" onclick={() => (invite = null)}>Cancel</Button>
			<Button
				disabled={saving || !/^\S+@\S+\.\S+$/.test(form.email.trim()) || form.roleIds.length === 0}
				onclick={sendInvite}>{saving ? 'Inviting…' : 'Invite'}</Button
			>
		{/snippet}
	</Modal>
{/if}

<Modal
	open={inviteLink !== ''}
	title="Invitation link"
	description="Share this link with the user. It is not shown again."
	size="lg"
	onclose={() => (inviteLink = '')}
>
	<p class="selectable rounded-md border bg-card px-3 py-2 font-mono text-xs break-all">{inviteLink}</p>
	{#snippet footer()}
		<Button variant="outline" onclick={() => copyText(inviteLink).then(() => toast.success('Copied'))}
			><Copy /> Copy</Button
		>
		<Button onclick={() => (inviteLink = '')}>Done</Button>
	{/snippet}
</Modal>
