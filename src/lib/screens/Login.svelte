<!-- Shown while disconnected: saved profiles as cards. -->
<script lang="ts">
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Lock from '@lucide/svelte/icons/lock';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import Server from '@lucide/svelte/icons/server';
	import Settings from '@lucide/svelte/icons/settings';
	import Star from '@lucide/svelte/icons/star';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import X from '@lucide/svelte/icons/x';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import IconButton from '$lib/components/IconButton.svelte';
	import type { ProfileView } from '$lib/ipc';
	import { app } from '$lib/services/app.svelte';
	import { confirm } from '$lib/services/confirm.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { cn } from '$lib/utils';
	import { workspace } from '$lib/workspace/workspace.svelte';
	import ProfileForm from './ProfileForm.svelte';

	let formOpen = $state(false);
	let editing = $state<ProfileView | null>(null);

	function create(): void {
		editing = null;
		formOpen = true;
	}

	function edit(profile: ProfileView): void {
		editing = profile;
		formOpen = true;
	}

	async function remove(view: ProfileView): Promise<void> {
		const ok = await confirm({
			title: `Delete “${view.profile.label}”?`,
			message:
				'The profile, its stored credentials and everything saved for this server (runbooks, bookmarks, backup templates…) will be deleted from this computer. The server itself is not touched.',
			confirmLabel: 'Delete profile',
			destructive: true
		});
		if (!ok) return;
		try {
			await app.deleteProfile(view.profile.id);
			toast.success('Profile deleted');
		} catch (error) {
			toast.error(error, 'Could not delete the profile');
		}
	}

	async function toggleDefault(view: ProfileView): Promise<void> {
		try {
			await app.setDefault(view.profile.isDefault ? null : view.profile.id);
		} catch (error) {
			toast.error(error);
		}
	}
</script>

<div class="relative flex h-full flex-col items-center overflow-y-auto px-6 py-12">
	<div class="absolute top-3 right-3">
		<IconButton label="Settings" side="left" onclick={() => workspace.openSettings()}><Settings /></IconButton
		>
	</div>

	<header class="mb-8 flex flex-col items-center gap-3 text-center">
		<img src="/logo.svg" alt="" class="size-16" />
		<div>
			<h1 class="text-2xl font-semibold tracking-tight">Jarvis Server Manager</h1>
			<p class="text-sm text-muted-foreground">Choose a server to connect to.</p>
		</div>
	</header>

	{#if app.connectError}
		<div
			class="mb-5 flex w-full max-w-3xl items-start gap-2.5 rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm"
			role="alert"
		>
			<CircleAlert class="mt-0.5 size-4 shrink-0 text-destructive" />
			<div class="selectable min-w-0 flex-1">
				<p class="font-medium">{app.connectError.title}</p>
				{#if app.connectError.details}
					<p class="text-xs break-words text-muted-foreground">{app.connectError.details}</p>
				{/if}
			</div>
			<button
				type="button"
				aria-label="Dismiss"
				class="text-muted-foreground hover:text-foreground"
				onclick={() => (app.connectError = null)}
			>
				<X class="size-4" />
			</button>
		</div>
	{/if}

	{#if !app.profilesLoaded}
		<LoaderCircle class="size-5 animate-spin text-muted-foreground" />
	{:else if app.profiles.length === 0}
		<div class="flex w-full max-w-md flex-col items-center gap-3 rounded-xl border bg-card p-8 text-center">
			<Server class="size-8 text-muted-foreground" />
			<div>
				<p class="font-medium">No servers yet</p>
				<p class="text-sm text-muted-foreground">
					Add the connection details of a Linux server to get started.
				</p>
			</div>
			<Button onclick={create}><Plus /> Create first profile</Button>
		</div>
	{:else}
		<div class="grid w-full max-w-3xl grid-cols-1 gap-3 sm:grid-cols-2">
			{#each app.profiles as view (view.profile.id)}
				{@const p = view.profile}
				{@const connecting = app.connectingId === p.id}
				<div
					class={cn(
						'group relative rounded-xl border bg-card transition-colors focus-within:border-primary/50 hover:border-primary/50',
						connecting && 'border-primary/60'
					)}
				>
					<button
						type="button"
						class="flex w-full items-center gap-3 rounded-xl p-4 text-left outline-none disabled:cursor-wait"
						disabled={app.connectingId !== null}
						onclick={() => app.connect(p.id)}
					>
						<span
							class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/12 text-primary"
						>
							{#if connecting}
								<LoaderCircle class="size-5 animate-spin" />
							{:else if p.authType === 'key'}
								<KeyRound class="size-5" />
							{:else}
								<Lock class="size-5" />
							{/if}
						</span>
						<span class="min-w-0 flex-1">
							<span class="flex items-center gap-2">
								<span class="truncate font-medium">{p.label}</span>
								{#if p.isDefault}<Badge variant="secondary">Default</Badge>{/if}
							</span>
							<span class="block truncate font-mono text-xs text-muted-foreground">
								{p.username}@{p.host}:{p.port}
							</span>
							{#if connecting}<span class="text-xs text-primary">Connecting…</span>{/if}
						</span>
					</button>
					<div
						class="absolute top-2 right-2 flex items-center rounded-md bg-card opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100"
					>
						<IconButton
							label={p.isDefault
								? 'Stop connecting automatically on launch'
								: 'Connect automatically on launch'}
							onclick={() => toggleDefault(view)}
						>
							<Star class={p.isDefault ? 'fill-warning text-warning' : ''} />
						</IconButton>
						<IconButton label="Edit" onclick={() => edit(view)}><Pencil /></IconButton>
						<IconButton label="Delete" onclick={() => remove(view)}><Trash2 /></IconButton>
					</div>
				</div>
			{/each}
		</div>
		<Button variant="outline" class="mt-5" onclick={create}><Plus /> New profile</Button>
	{/if}
</div>

<ProfileForm bind:open={formOpen} {editing} />
