<!-- Create or edit a restic repository configuration. Secrets go to the keyring. -->
<script lang="ts" module>
	import type { ResticRepo } from '$lib/ipc';

	export const KINDS = [
		{ value: 'local', label: 'Folder on the server' },
		{ value: 's3', label: 'S3-compatible storage' },
		{ value: 'b2', label: 'Backblaze B2' },
		{ value: 'sftp', label: 'SFTP server' },
		{ value: 'rest', label: 'REST server' },
		{ value: 'rclone', label: 'rclone remote' }
	] as const;

	export const blankRepo = (): ResticRepo => ({
		id: '',
		name: '',
		kind: 'local',
		repository: '',
		fields: {},
		envNames: [],
		sudo: false
	});

	/** The repository string restic expects, built from the form fields. */
	export function repositoryString(kind: string, f: Record<string, string>): string {
		const get = (key: string) => (f[key] ?? '').trim();
		const path = get('path');
		switch (kind) {
			case 'local':
				return path;
			case 's3': {
				if (!get('bucket')) return '';
				const endpoint = get('endpoint') || 's3.amazonaws.com';
				const base = /^https?:\/\//.test(endpoint) ? endpoint : `https://${endpoint}`;
				return `s3:${base.replace(/\/+$/, '')}/${get('bucket')}${path ? `/${path.replace(/^\/+/, '')}` : ''}`;
			}
			case 'b2':
				return get('bucket') ? `b2:${get('bucket')}:${path.replace(/^\/+/, '')}` : '';
			case 'sftp': {
				if (!get('host') || !path) return '';
				const user = get('user') ? `${get('user')}@` : '';
				const port = get('port');
				return port && port !== '22'
					? `sftp://${user}${get('host')}:${port}/${path}`
					: `sftp:${user}${get('host')}:${path}`;
			}
			case 'rest':
				return get('url') ? `rest:${get('url')}` : '';
			case 'rclone':
				return get('remote') ? `rclone:${get('remote')}:${path}` : '';
			default:
				return '';
		}
	}
</script>

<script lang="ts">
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { api } from '$lib/ipc';
	import { toast } from '$lib/services/toast.svelte';

	interface Props {
		repo: ResticRepo | null;
		onsaved: (repo: ResticRepo) => void;
	}

	let { repo = $bindable(), onsaved }: Props = $props();

	interface EnvRow {
		name: string;
		value: string;
		/** Already stored: the value is kept unless a new one is typed. */
		stored: boolean;
	}

	let draft = $state<ResticRepo | null>(null);
	let password = $state('');
	let accessKey = $state('');
	let secretKey = $state('');
	let env = $state<EnvRow[]>([]);
	let remotes = $state<string[] | null>(null);
	let detecting = $state(false);
	let saving = $state(false);

	$effect(() => {
		if (!repo) {
			draft = null;
			return;
		}
		draft = structuredClone($state.snapshot(repo));
		password = accessKey = secretKey = '';
		env = draft.envNames.map((name) => ({ name, value: '', stored: true }));
		remotes = null;
	});

	const isNew = $derived(!draft?.id);
	const repository = $derived(draft ? repositoryString(draft.kind, draft.fields) : '');
	const keyLabels = $derived(
		draft?.kind === 's3'
			? ['Access key ID', 'Secret access key']
			: draft?.kind === 'b2'
				? ['Account / key ID', 'Application key']
				: draft?.kind === 'rest'
					? ['Username (optional)', 'Password']
					: null
	);
	const error = $derived.by(() => {
		if (!draft) return null;
		if (!draft.name.trim()) return 'Enter a name.';
		if (!repository) return 'The repository location is incomplete.';
		if (draft.kind === 'local' && !repository.startsWith('/')) return 'The folder must be an absolute path.';
		if (isNew && !password) return 'Enter the repository password.';
		const names = env.map((e) => e.name.trim());
		if (names.some((n) => !/^[A-Za-z_][A-Za-z0-9_]*$/.test(n)))
			return 'Environment variable names may only contain letters, digits and _.';
		if (new Set(names).size !== names.length) return 'Environment variable names must be unique.';
		return null;
	});

	async function detectRemotes(): Promise<void> {
		if (!draft) return;
		detecting = true;
		try {
			remotes = await api.resticRcloneRemotes(draft.sudo);
			if (remotes.length === 0) toast.warning('rclone has no remotes configured on this server');
			else if (!draft.fields.remote) draft.fields.remote = remotes[0];
		} catch (e) {
			toast.error(e, 'Could not list rclone remotes');
		} finally {
			detecting = false;
		}
	}

	async function save(): Promise<void> {
		if (!draft || error) return;
		saving = true;
		try {
			const next = { ...$state.snapshot(draft), repository, envNames: env.map((e) => e.name.trim()) };
			const saved = await api.resticRepoSave(next, {
				password: password ? password : null,
				accessKey: accessKey || isNew ? accessKey : null,
				secretKey: secretKey || isNew ? secretKey : null,
				env: env.filter((e) => !e.stored || e.value).map((e) => [e.name.trim(), e.value])
			});
			repo = null;
			onsaved(saved);
		} catch (e) {
			toast.error(e, 'Could not save the repository');
		} finally {
			saving = false;
		}
	}
</script>

{#if draft}
	<Modal
		open
		title={isNew ? 'Add a restic repository' : 'Edit restic repository'}
		description="Passwords and keys are stored in your system keyring and passed to restic through its environment."
		size="lg"
		onclose={() => (repo = null)}
	>
		<div class="grid grid-cols-2 gap-3">
			<Field label="Name">
				<Input bind:value={draft.name} placeholder="e.g. Offsite backups" autofocus />
			</Field>
			<Field label="Type">
				<SelectField bind:value={draft.kind} options={KINDS} />
			</Field>

			{#if draft.kind === 'local'}
				<Field label="Folder" class="col-span-2"
					><PathInput bind:value={draft.fields.path} dirsOnly placeholder="/srv/restic-repo" /></Field
				>
			{:else if draft.kind === 's3'}
				<Field label="Endpoint" hint="Leave empty for Amazon S3."
					><Input bind:value={draft.fields.endpoint} placeholder="s3.example.com" spellcheck="false" /></Field
				>
				<Field label="Bucket"><Input bind:value={draft.fields.bucket} spellcheck="false" /></Field>
				<Field label="Path in bucket" class="col-span-2" hint="Optional."
					><Input bind:value={draft.fields.path} spellcheck="false" /></Field
				>
			{:else if draft.kind === 'b2'}
				<Field label="Bucket"><Input bind:value={draft.fields.bucket} spellcheck="false" /></Field>
				<Field label="Path in bucket" hint="Optional."
					><Input bind:value={draft.fields.path} spellcheck="false" /></Field
				>
			{:else if draft.kind === 'sftp'}
				<Field label="Host"><Input bind:value={draft.fields.host} spellcheck="false" /></Field>
				<Field label="Port"><Input bind:value={draft.fields.port} placeholder="22" /></Field>
				<Field label="User"><Input bind:value={draft.fields.user} spellcheck="false" /></Field>
				<Field label="Path"
					><Input bind:value={draft.fields.path} placeholder="/backups/restic" spellcheck="false" /></Field
				>
				<p class="col-span-2 text-xs text-muted-foreground">
					restic logs in with the server’s own SSH key: the server must be able to run <code
						>ssh user@host</code
					> without a password.
				</p>
			{:else if draft.kind === 'rest'}
				<Field label="URL" class="col-span-2"
					><Input
						bind:value={draft.fields.url}
						placeholder="https://backup.example.com:8000/"
						spellcheck="false"
					/></Field
				>
			{:else if draft.kind === 'rclone'}
				<Field label="Remote" class="col-span-2">
					<div class="flex gap-2">
						{#if remotes?.length}
							<SelectField bind:value={draft.fields.remote} options={remotes} />
						{:else}
							<Input bind:value={draft.fields.remote} placeholder="remote name" spellcheck="false" />
						{/if}
						<Button type="button" variant="outline" disabled={detecting} onclick={detectRemotes}
							>{detecting ? 'Detecting…' : 'Detect remotes'}</Button
						>
					</div>
				</Field>
				<Field label="Path on the remote" class="col-span-2"
					><Input bind:value={draft.fields.path} placeholder="bucket/restic" spellcheck="false" /></Field
				>
			{/if}

			{#if repository}
				<p class="selectable col-span-2 font-mono text-xs break-all text-muted-foreground">{repository}</p>
			{/if}

			<Field
				label="Repository password"
				class="col-span-2"
				hint={isNew
					? 'Encrypts the repository. It cannot be recovered if lost.'
					: 'Leave empty to keep the stored password.'}
			>
				<Input type="password" bind:value={password} autocomplete="new-password" />
			</Field>
			{#if keyLabels}
				<Field label={keyLabels[0]} hint={isNew ? undefined : 'Empty keeps the stored value.'}
					><Input bind:value={accessKey} spellcheck="false" autocomplete="off" /></Field
				>
				<Field label={keyLabels[1]} hint={isNew ? undefined : 'Empty keeps the stored value.'}
					><Input type="password" bind:value={secretKey} autocomplete="new-password" /></Field
				>
			{/if}

			<div class="col-span-2">
				<div class="mb-1 flex items-center gap-2">
					<p class="text-sm font-medium">Extra environment variables</p>
					<Button
						type="button"
						variant="ghost"
						size="xs"
						onclick={() => env.push({ name: '', value: '', stored: false })}><Plus /> Add</Button
					>
				</div>
				{#each env as row, i (i)}
					<div class="mb-1.5 flex items-center gap-2">
						<Input
							bind:value={row.name}
							disabled={row.stored}
							placeholder="NAME"
							class="w-56 font-mono text-xs"
							spellcheck="false"
						/>
						<Input
							type="password"
							bind:value={row.value}
							placeholder={row.stored ? 'stored (type to replace)' : 'value'}
							class="flex-1 font-mono text-xs"
							autocomplete="new-password"
						/>
						<IconButton label="Remove variable" onclick={() => env.splice(i, 1)}><X /></IconButton>
					</div>
				{:else}
					<p class="text-xs text-muted-foreground">None. Values are treated as secrets.</p>
				{/each}
			</div>

			<label class="col-span-2 flex items-center gap-2 text-sm">
				<Checkbox bind:checked={draft.sudo} /> Run restic as root (needed to back up files only root can read)
			</label>
		</div>
		{#snippet footer()}
			{#if error}<p class="mr-auto text-xs text-muted-foreground">{error}</p>{/if}
			<Button variant="outline" onclick={() => (repo = null)}>Cancel</Button>
			<Button disabled={!!error || saving} onclick={save}>{saving ? 'Saving…' : 'Save'}</Button>
		{/snippet}
	</Modal>
{/if}
