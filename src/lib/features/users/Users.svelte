<script lang="ts">
	import KeyRound from '@lucide/svelte/icons/key-round';
	import Lock from '@lucide/svelte/icons/lock';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import UsersRound from '@lucide/svelte/icons/users-round';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Page from '$lib/components/Page.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import { Textarea } from '$lib/components/ui/textarea';
	import { api, type Group, type User } from '$lib/ipc';
	import { confirm, prompt } from '$lib/services/confirm.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';
	import { matches } from '$lib/utils';
	import type { TabProps } from '$lib/workspace/registry';

	let { visible }: TabProps = $props();

	const accounts = resource((io) => io.accountsList());
	const busy = new Busy();
	let view = $state<'users' | 'groups'>('users');
	let showSystem = $state(false);
	let search = $state('');
	let createOpen = $state(false);
	let newUser = $state({ name: '', shell: '/bin/bash', comment: '', password: '' });
	let submitted = $state(false);
	let groupsFor = $state<User | null>(null);
	let chosenGroups = $state<string[]>([]);
	let groupFilter = $state('');
	let keysFor = $state<User | null>(null);
	let keysText = $state('');

	autoLoad(accounts, () => visible);

	const PRIVILEGED = ['sudo', 'wheel', 'docker'];
	const current = $derived(accounts.data?.currentUser ?? '');
	const users = $derived(
		(accounts.data?.users ?? []).filter((u) => showSystem || (u.uid >= 1000 && u.uid < 65534) || u.uid === 0)
	);
	const groups = $derived(
		(accounts.data?.groups ?? []).filter(
			(g) => showSystem || (g.gid >= 1000 && g.gid < 65534) || PRIVILEGED.includes(g.name)
		)
	);

	const userColumns: Column<User>[] = [
		{ key: 'name', label: 'User', value: (u) => u.name, class: 'w-44 max-w-44' },
		{ key: 'uid', label: 'UID', value: (u) => u.uid, align: 'right', class: 'w-20 tabular' },
		{ key: 'home', label: 'Home', value: (u) => u.home, mono: true, class: 'max-w-56' },
		{ key: 'shell', label: 'Shell', value: (u) => u.shell, mono: true, class: 'w-44 max-w-44' },
		{ key: 'groups', label: 'Groups', value: (u) => u.groups.join(', '), class: 'max-w-0 w-full' }
	];
	const groupColumns: Column<Group>[] = [
		{ key: 'name', label: 'Group', value: (g) => g.name, class: 'w-52 max-w-52' },
		{ key: 'gid', label: 'GID', value: (g) => g.gid, align: 'right', class: 'w-20 tabular' },
		{ key: 'members', label: 'Members', value: (g) => g.members.join(', '), class: 'max-w-0 w-full' }
	];

	async function run(
		key: string,
		reason: string,
		action: () => Promise<unknown>,
		done: string
	): Promise<boolean> {
		const ok = await busy.run(
			key,
			() => sudo.describe(reason, action),
			(e) => toast.error(e)
		);
		if (ok) toast.success(done);
		await accounts.load(true);
		return ok;
	}

	const nameError = $derived(
		/^[a-z_][a-z0-9_-]{0,31}$/.test(newUser.name)
			? null
			: 'Lowercase letters, digits, “-” and “_”; must start with a letter.'
	);

	async function createUser(): Promise<void> {
		submitted = true;
		if (nameError) return;
		if (
			await run(
				'create',
				`Create user ${newUser.name}`,
				() => api.userCreate($state.snapshot(newUser)),
				`User ${newUser.name} created`
			)
		) {
			createOpen = false;
		}
	}

	async function deleteUser(user: User): Promise<void> {
		const ok = await confirm({
			title: `Delete user “${user.name}”?`,
			message: `The account is removed from the server. You are asked next whether to keep its home directory (${user.home}).`,
			typeToConfirm: user.name,
			confirmLabel: 'Delete user',
			destructive: true
		});
		if (!ok) return;
		const alsoHome = await confirm({
			title: 'Also delete the home directory?',
			message: `${user.home} and the user’s mail spool would be deleted permanently.`,
			confirmLabel: 'Delete home directory',
			cancelLabel: 'Keep it',
			destructive: true
		});
		await run(
			user.name,
			`Delete user ${user.name}`,
			() => api.userDelete(user.name, alsoHome),
			`User ${user.name} deleted`
		);
	}

	async function changePassword(user: User): Promise<void> {
		const password = await prompt({
			title: `New password for ${user.name}`,
			label: 'Password',
			password: true,
			validate: (v) => (v.length < 1 ? 'Enter a password.' : null)
		});
		if (password === null) return;
		await run(
			user.name,
			`Change the password of ${user.name}`,
			() => api.userSetPassword(user.name, password),
			`Password of ${user.name} changed`
		);
	}

	const toggleLock = (user: User, locked: boolean) =>
		run(
			user.name,
			`${locked ? 'Lock' : 'Unlock'} ${user.name}`,
			() => api.userSetLocked(user.name, locked),
			`${user.name} ${locked ? 'locked' : 'unlocked'}`
		);

	function openGroups(user: User): void {
		// The primary group (first) is fixed; membership covers supplementary groups.
		chosenGroups = user.groups.slice(1);
		groupFilter = '';
		groupsFor = user;
	}

	async function saveGroups(): Promise<void> {
		const user = groupsFor;
		if (!user) return;
		if (
			await run(
				user.name,
				`Change the groups of ${user.name}`,
				() => api.userSetGroups(user.name, chosenGroups),
				`Groups of ${user.name} updated`
			)
		) {
			groupsFor = null;
		}
	}

	async function openKeys(user: User): Promise<void> {
		try {
			keysText = await sudo.describe(`Read the SSH keys of ${user.name}`, () => api.userKeysGet(user.name));
			keysFor = user;
		} catch (error) {
			toast.error(error);
		}
	}

	async function saveKeys(): Promise<void> {
		const user = keysFor;
		if (!user) return;
		if (
			await run(
				user.name,
				`Save the SSH keys of ${user.name}`,
				() => api.userKeysSet(user.name, keysText),
				'Authorized keys saved'
			)
		) {
			keysFor = null;
		}
	}

	async function createGroup(): Promise<void> {
		const name = await prompt({
			title: 'New group',
			label: 'Group name',
			validate: (v) => (/^[a-z_][a-z0-9_-]{0,31}$/.test(v) ? null : 'Lowercase letters, digits, “-” and “_”.')
		});
		if (name)
			await run('group', `Create group ${name}`, () => api.groupCreate(name), `Group ${name} created`);
	}

	async function deleteGroup(group: Group): Promise<void> {
		const ok = await confirm({
			title: `Delete group “${group.name}”?`,
			message: group.members.length
				? `Members (${group.members.join(', ')}) lose the access this group gave them.`
				: undefined,
			confirmLabel: 'Delete group',
			destructive: true
		});
		if (ok)
			await run(
				group.name,
				`Delete group ${group.name}`,
				() => api.groupDelete(group.name),
				`Group ${group.name} deleted`
			);
	}

	export const refresh = () => accounts.refresh();
</script>

<Page scroll={false}>
	{#snippet toolbar()}
		<SearchInput bind:value={search} placeholder="Search…" class="w-56" />
		<label class="flex items-center gap-1.5 text-xs text-muted-foreground"
			><Switch size="sm" bind:checked={showSystem} /> Show system accounts (UID &lt; 1000)</label
		>
		<span class="flex-1"></span>
		{#if view === 'users'}
			<Button
				size="sm"
				onclick={() => {
					newUser = {
						name: '',
						shell: accounts.data?.shells.includes('/bin/bash')
							? '/bin/bash'
							: (accounts.data?.shells[0] ?? '/bin/sh'),
						comment: '',
						password: ''
					};
					submitted = false;
					createOpen = true;
				}}><Plus /> Create user</Button
			>
		{:else}
			<Button size="sm" onclick={createGroup}><Plus /> Create group</Button>
		{/if}
		<RefreshControl onrefresh={accounts.refresh} loading={accounts.loading} />
	{/snippet}
	{#snippet header()}
		<SubTabs
			bind:value={view}
			items={[
				{ id: 'users', label: 'Users', count: users.length },
				{ id: 'groups', label: 'Groups', count: groups.length }
			]}
		/>
	{/snippet}

	{#if view === 'users'}
		<DataTable
			rows={users}
			columns={userColumns}
			rowKey={(u) => u.name}
			{search}
			busy={busy.keys}
			loading={accounts.loading}
			error={accounts.error}
			onretry={accounts.refresh}
			empty="No users"
			class="flex-1"
		>
			{#snippet cell(user, column)}
				{#if column.key === 'name'}
					<span class="flex items-center gap-1.5">
						<span class="truncate font-medium">{user.name}</span>
						{#if user.name === current}<span class="rounded bg-primary/15 px-1 text-[10px] text-primary"
								>you</span
							>{/if}
						{#if user.locked}<Lock class="size-3 shrink-0 text-warning" aria-label="Locked" />{/if}
					</span>
				{:else}
					{column.value?.(user)}
				{/if}
			{/snippet}
			{#snippet actions(user)}
				<IconButton label="Group membership" onclick={() => openGroups(user)}><UsersRound /></IconButton>
				<IconButton label="Change password" onclick={() => changePassword(user)}><KeyRound /></IconButton>
				<IconButton
					label="Delete user"
					disabled={user.uid === 0 || user.name === current}
					onclick={() => deleteUser(user)}><Trash2 /></IconButton
				>
			{/snippet}
			{#snippet menu(user)}
				<ContextMenu.Item onclick={() => openGroups(user)}>Group membership…</ContextMenu.Item>
				<ContextMenu.Item onclick={() => changePassword(user)}>Change password…</ContextMenu.Item>
				<ContextMenu.Item onclick={() => openKeys(user)}>SSH authorized keys…</ContextMenu.Item>
				<ContextMenu.Separator />
				{#if user.locked}
					<ContextMenu.Item onclick={() => toggleLock(user, false)}>Unlock account</ContextMenu.Item>
				{:else}
					<ContextMenu.Item
						disabled={user.uid === 0 || user.name === current}
						onclick={() => toggleLock(user, true)}>Lock account</ContextMenu.Item
					>
				{/if}
				<ContextMenu.Item
					variant="destructive"
					disabled={user.uid === 0 || user.name === current}
					onclick={() => deleteUser(user)}>Delete user…</ContextMenu.Item
				>
			{/snippet}
		</DataTable>
	{:else}
		<DataTable
			rows={groups}
			columns={groupColumns}
			rowKey={(g) => g.name}
			{search}
			busy={busy.keys}
			loading={accounts.loading}
			error={accounts.error}
			onretry={accounts.refresh}
			empty="No groups"
			class="flex-1"
		>
			{#snippet actions(group)}
				<IconButton
					label="Delete group"
					disabled={['root', 'sudo', 'wheel'].includes(group.name)}
					onclick={() => deleteGroup(group)}><Trash2 /></IconButton
				>
			{/snippet}
		</DataTable>
	{/if}
</Page>

<Modal
	bind:open={createOpen}
	title="Create user"
	description="A home directory is created for the new account."
	size="md"
>
	<form
		id="user-form"
		class="flex flex-col gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void createUser();
		}}
	>
		<Field label="Username" required error={submitted ? nameError : null}
			><Input bind:value={newUser.name} spellcheck="false" autocomplete="off" autofocus /></Field
		>
		<Field label="Full name"><Input bind:value={newUser.comment} /></Field>
		<Field label="Shell"
			><SelectField bind:value={newUser.shell} options={accounts.data?.shells ?? ['/bin/sh']} /></Field
		>
		<Field label="Password" hint="Optional. Without one the user can only log in with an SSH key."
			><Input type="password" bind:value={newUser.password} autocomplete="new-password" /></Field
		>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
		<Button type="submit" form="user-form" disabled={busy.has('create')}>Create user</Button>
	{/snippet}
</Modal>

{#if groupsFor}
	{@const user = groupsFor}
	<Modal
		open
		title="Groups of {user.name}"
		description="Primary group: {user.groups[0] ?? '—'}"
		size="md"
		onclose={() => (groupsFor = null)}
	>
		<SearchInput bind:value={groupFilter} placeholder="Filter groups…" class="mb-2 w-full" />
		<ul class="max-h-72 overflow-y-auto rounded-lg border">
			{#each (accounts.data?.groups ?? []).filter((g) => g.name !== user.groups[0] && matches(groupFilter, g.name)) as group (group.name)}
				<li>
					<label class="flex items-center gap-2 px-3 py-1.5 text-sm hover:bg-muted/50">
						<Checkbox
							checked={chosenGroups.includes(group.name)}
							onCheckedChange={(v) =>
								(chosenGroups = v
									? [...chosenGroups, group.name]
									: chosenGroups.filter((g) => g !== group.name))}
						/>
						<span class={PRIVILEGED.includes(group.name) ? 'font-medium' : ''}>{group.name}</span>
						{#if PRIVILEGED.includes(group.name)}<span class="text-xs text-warning">privileged</span>{/if}
						<span class="ml-auto text-xs text-muted-foreground tabular">{group.gid}</span>
					</label>
				</li>
			{/each}
		</ul>
		{#snippet footer()}
			<Button variant="outline" onclick={() => (groupsFor = null)}>Cancel</Button>
			<Button disabled={busy.has(user.name)} onclick={saveGroups}>Save</Button>
		{/snippet}
	</Modal>
{/if}

{#if keysFor}
	{@const user = keysFor}
	<Modal
		open
		title="Authorized SSH keys of {user.name}"
		description="One public key per line ({user.home}/.ssh/authorized_keys)."
		size="xl"
		onclose={() => (keysFor = null)}
	>
		<Textarea
			bind:value={keysText}
			rows={10}
			class="font-mono text-xs"
			spellcheck="false"
			placeholder="ssh-ed25519 AAAA… user@host"
		/>
		{#snippet footer()}
			<Button variant="outline" onclick={() => (keysFor = null)}>Cancel</Button>
			<Button disabled={busy.has(user.name)} onclick={saveKeys}>Save keys</Button>
		{/snippet}
	</Modal>
{/if}
