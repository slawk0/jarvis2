<!-- Create / edit a server profile. -->
<script lang="ts">
	import { open as pickFile } from '@tauri-apps/plugin-dialog';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import Field from '$lib/components/Field.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import { api, toIpcError, type AuthType, type ProfileView } from '$lib/ipc';
	import { app } from '$lib/services/app.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { cn } from '$lib/utils';

	interface Props {
		open: boolean;
		/** Profile to edit; `null` creates a new one. */
		editing: ProfileView | null;
	}

	let { open = $bindable(false), editing }: Props = $props();

	let label = $state('');
	let host = $state('');
	let port = $state('22');
	let username = $state('root');
	let authType = $state<AuthType>('password');
	let password = $state('');
	let keyPath = $state('');
	let passphrase = $state('');
	let clearSecrets = $state(false);
	let submitted = $state(false);
	let saving = $state(false);
	let keyError = $state<string | null>(null);
	let formError = $state<string | null>(null);

	// Fill the form each time the dialog opens.
	$effect(() => {
		if (!open) return;
		const p = editing?.profile;
		label = p?.label ?? '';
		host = p?.host ?? '';
		port = String(p?.port ?? 22);
		username = p?.username ?? 'root';
		authType = p?.authType ?? 'password';
		keyPath = p?.keyPath ?? '';
		password = '';
		passphrase = '';
		clearSecrets = false;
		submitted = false;
		keyError = null;
		formError = null;
	});

	const hasStoredSecret = $derived(
		editing !== null && (authType === 'password' ? editing.hasPassword : editing.hasPassphrase)
	);

	const errors = $derived.by(() => {
		const e: Record<string, string> = {};
		if (!label.trim()) e.label = 'Give the profile a name.';
		if (!host.trim()) e.host = 'Enter a host name or IP address.';
		else if (/\s/.test(host.trim())) e.host = 'A host name cannot contain spaces.';
		const p = Number(port);
		if (!Number.isInteger(p) || p < 1 || p > 65535) e.port = 'Port must be 1–65535.';
		if (!username.trim()) e.username = 'Enter the user to log in as.';
		if (authType === 'password' && !password && !(editing?.hasPassword && !clearSecrets)) {
			e.password = 'Enter the password.';
		}
		if (authType === 'key' && !keyPath.trim()) e.keyPath = 'Choose a private key file.';
		return e;
	});
	const shown = (field: string) => (submitted ? (errors[field] ?? null) : null);

	async function browse(): Promise<void> {
		const picked = await pickFile({
			title: 'Choose a private key',
			multiple: false,
			directory: false,
			defaultPath: (await api.defaultKeyDir()) ?? undefined
		});
		if (typeof picked !== 'string') return;
		await resolveKey(picked);
	}

	async function resolveKey(path: string): Promise<boolean> {
		keyError = null;
		try {
			keyPath = await api.profileResolveKey(path);
			return true;
		} catch (raw) {
			keyError = toIpcError(raw).details ?? toIpcError(raw).message;
			return false;
		}
	}

	async function save(): Promise<void> {
		submitted = true;
		formError = null;
		if (Object.keys(errors).length > 0) return;
		if (authType === 'key' && !(await resolveKey(keyPath))) return;
		saving = true;
		try {
			await app.saveProfile({
				id: editing?.profile.id ?? null,
				label: label.trim(),
				host: host.trim(),
				port: Number(port),
				username: username.trim(),
				authType,
				keyPath: authType === 'key' ? keyPath.trim() : null,
				password: authType === 'password' && password ? password : null,
				passphrase: authType === 'key' && passphrase ? passphrase : null,
				clearSecrets
			});
			toast.success(editing ? 'Profile updated' : 'Profile created');
			open = false;
		} catch (raw) {
			formError = toIpcError(raw).message;
		} finally {
			saving = false;
		}
	}
</script>

<Modal bind:open title={editing ? 'Edit profile' : 'New profile'} size="md">
	<form
		id="profile-form"
		class="flex flex-col gap-3.5"
		novalidate
		onsubmit={(e) => {
			e.preventDefault();
			void save();
		}}
	>
		<Field label="Label" for="pf-label" required error={shown('label')}>
			<Input id="pf-label" bind:value={label} placeholder="Production web server" autofocus />
		</Field>
		<div class="grid grid-cols-[1fr_6rem] gap-3">
			<Field label="Host" for="pf-host" required error={shown('host')}>
				<Input
					id="pf-host"
					bind:value={host}
					placeholder="server.example.com"
					spellcheck="false"
					autocomplete="off"
				/>
			</Field>
			<Field label="Port" for="pf-port" error={shown('port')}>
				<Input id="pf-port" bind:value={port} inputmode="numeric" />
			</Field>
		</div>
		<Field label="User" for="pf-user" error={shown('username')}>
			<Input id="pf-user" bind:value={username} spellcheck="false" autocomplete="off" />
		</Field>

		<Field label="Authentication">
			<div class="grid grid-cols-2 gap-1 rounded-lg bg-muted p-1" role="radiogroup">
				{#each [{ id: 'password', label: 'Password' }, { id: 'key', label: 'Private key' }] as option (option.id)}
					<button
						type="button"
						role="radio"
						aria-checked={authType === option.id}
						class={cn(
							'h-7 rounded-md text-sm transition-colors',
							authType === option.id
								? 'bg-background font-medium shadow-sm'
								: 'text-muted-foreground hover:text-foreground'
						)}
						onclick={() => (authType = option.id as AuthType)}
					>
						{option.label}
					</button>
				{/each}
			</div>
		</Field>

		{#if authType === 'password'}
			<Field
				label="Password"
				for="pf-password"
				error={shown('password')}
				hint={editing?.hasPassword ? 'Leave empty to keep the stored password.' : undefined}
			>
				<Input id="pf-password" type="password" bind:value={password} autocomplete="off" />
			</Field>
		{:else}
			<Field label="Private key file" for="pf-key" required error={keyError ?? shown('keyPath')}>
				<div class="flex gap-2">
					<Input
						id="pf-key"
						bind:value={keyPath}
						placeholder="~/.ssh/id_ed25519"
						spellcheck="false"
						autocomplete="off"
					/>
					<Button variant="outline" onclick={browse}>Browse…</Button>
				</div>
			</Field>
			<Field
				label="Key passphrase"
				for="pf-passphrase"
				hint={editing?.hasPassphrase
					? 'Leave empty to keep the stored passphrase.'
					: 'Only if the key is encrypted.'}
			>
				<Input id="pf-passphrase" type="password" bind:value={passphrase} autocomplete="off" />
			</Field>
		{/if}

		{#if hasStoredSecret}
			<label class="flex items-center gap-2 text-xs text-muted-foreground">
				<Checkbox bind:checked={clearSecrets} />
				Clear the stored {authType === 'password' ? 'password' : 'passphrase'} from the system keyring
			</label>
		{/if}

		{#if formError}
			<p class="text-xs text-destructive" role="alert">{formError}</p>
		{/if}
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (open = false)}>Cancel</Button>
		<Button type="submit" form="profile-form" disabled={saving}>
			{saving ? 'Saving…' : editing ? 'Save changes' : 'Create profile'}
		</Button>
	{/snippet}
</Modal>
