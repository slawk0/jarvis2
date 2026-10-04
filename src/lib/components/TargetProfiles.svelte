<script lang="ts" module>
	import type { ExecTarget } from '$lib/ipc';

	/** The backend execution target for a saved profile. */
	export function execTarget(profile: { kind: 'host' | 'container'; container: string }): ExecTarget {
		return profile.kind === 'container'
			? { kind: 'container', container: profile.container }
			: { kind: 'host' };
	}
</script>

<!--
	Reusable "target profiles": named places a tool runs (the host or a Docker
	container) plus feature-specific fields. Shows a setup card when there are
	none yet, otherwise a selector and a manage dialog.
-->
<script lang="ts" generics="T extends ExecTargetProfile">
	import type { Snippet } from 'svelte';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Plus from '@lucide/svelte/icons/plus';
	import Settings2 from '@lucide/svelte/icons/settings-2';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { apiQuiet } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import type { ExecTargetProfile } from '$lib/services/profile-data';
	import { toast } from '$lib/services/toast.svelte';
	import { uid } from '$lib/utils';
	import Field from './Field.svelte';
	import IconButton from './IconButton.svelte';
	import Modal from './Modal.svelte';
	import SelectField from './SelectField.svelte';

	interface Props {
		/** What these profiles are, e.g. "Nginx target". */
		noun: string;
		description: string;
		profiles: T[];
		selectedId: string;
		/** A new profile with default values. */
		blank: () => T;
		save: (profiles: T[]) => Promise<void>;
		/** Feature-specific form fields for the profile being edited. */
		fields?: Snippet<[T]>;
		/** Return an error message to block saving. */
		validate?: (profile: T) => string | null;
	}

	let {
		noun,
		description,
		profiles,
		selectedId = $bindable(),
		blank,
		save,
		fields,
		validate
	}: Props = $props();

	let manageOpen = $state(false);
	let draft = $state<T | null>(null);
	let containers = $state<string[]>([]);
	let saving = $state(false);

	const error = $derived(
		!draft
			? null
			: !draft.name.trim()
				? 'Enter a name.'
				: draft.kind === 'container' && !draft.container.trim()
					? 'Choose a container.'
					: (validate?.(draft) ?? null)
	);

	function edit(profile: T | null): void {
		draft = structuredClone($state.snapshot(profile ?? blank())) as T;
		apiQuiet.dockerContainers().then(
			(list) => (containers = list.map((c) => c.name)),
			() => (containers = [])
		);
	}

	async function persist(next: T[]): Promise<boolean> {
		saving = true;
		try {
			await save(next);
			return true;
		} catch (e) {
			toast.error(e, 'Could not save');
			return false;
		} finally {
			saving = false;
		}
	}

	async function submit(): Promise<void> {
		if (!draft || error) return;
		const entry = { ...$state.snapshot(draft), name: draft.name.trim(), id: draft.id || uid() } as T;
		const next = draft.id ? profiles.map((p) => (p.id === draft!.id ? entry : p)) : [...profiles, entry];
		if (await persist(next)) {
			selectedId = entry.id;
			draft = null;
		}
	}

	async function remove(profile: T): Promise<void> {
		const ok = await confirm({
			title: `Delete “${profile.name}”?`,
			message: 'Only this saved profile is removed; nothing on the server changes.',
			confirmLabel: 'Delete',
			destructive: true
		});
		if (!ok) return;
		if ((await persist(profiles.filter((p) => p.id !== profile.id))) && selectedId === profile.id) {
			selectedId = profiles.find((p) => p.id !== profile.id)?.id ?? '';
		}
	}
</script>

{#snippet form(d: T)}
	<form
		id="target-profile-form"
		class="flex flex-col gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void submit();
		}}
	>
		<Field label="Name" required><Input bind:value={d.name} placeholder="Production" autofocus /></Field>
		<Field label="Runs on">
			<SelectField
				bind:value={d.kind}
				options={[
					{ value: 'host', label: 'The server itself' },
					{ value: 'container', label: 'A Docker container' }
				]}
			/>
		</Field>
		{#if d.kind === 'container'}
			<Field label="Container" required>
				{#if containers.length}
					<SelectField
						bind:value={d.container}
						options={[...new Set([...containers, ...(d.container ? [d.container] : [])])]}
						placeholder="Choose a container…"
					/>
				{:else}
					<Input bind:value={d.container} placeholder="container name" spellcheck="false" />
				{/if}
			</Field>
		{/if}
		{@render fields?.(d)}
		{#if error}<p class="text-xs text-muted-foreground">{error}</p>{/if}
	</form>
{/snippet}

{#if profiles.length === 0}
	<div class="mx-auto flex h-full max-w-lg flex-col justify-center gap-4 p-6">
		<div>
			<h2 class="text-base font-semibold">Set up your first {noun.toLowerCase()}</h2>
			<p class="text-sm text-muted-foreground">{description}</p>
		</div>
		{#if draft}
			<div class="rounded-xl border bg-card p-4">
				{@render form(draft)}
				<div class="mt-4 flex justify-end">
					<Button type="submit" form="target-profile-form" disabled={saving || error !== null}>Save</Button>
				</div>
			</div>
		{:else}
			<div><Button onclick={() => edit(null)}><Plus /> Add {noun.toLowerCase()}</Button></div>
		{/if}
	</div>
{:else}
	<div class="flex items-center gap-1.5">
		<SelectField
			bind:value={selectedId}
			class="w-56"
			options={profiles.map((p) => ({
				value: p.id,
				label: `${p.name} · ${p.kind === 'container' ? p.container : 'host'}`
			}))}
		/>
		<IconButton label="Manage {noun.toLowerCase()}s" variant="outline" onclick={() => (manageOpen = true)}
			><Settings2 /></IconButton
		>
	</div>

	<Modal bind:open={manageOpen} title="{noun}s" size="md" onclose={() => (draft = null)}>
		{#if draft}
			{@render form(draft)}
		{:else}
			<ul class="divide-y rounded-lg border">
				{#each profiles as profile (profile.id)}
					<li class="flex items-center gap-2 px-3 py-2">
						<div class="min-w-0 flex-1">
							<p class="truncate text-sm font-medium">{profile.name}</p>
							<p class="truncate text-xs text-muted-foreground">
								{profile.kind === 'container' ? `container ${profile.container}` : 'on the server'}
							</p>
						</div>
						<IconButton label="Edit" onclick={() => edit(profile)}><Pencil /></IconButton>
						<IconButton label="Delete" onclick={() => remove(profile)}><Trash2 /></IconButton>
					</li>
				{/each}
			</ul>
		{/if}
		{#snippet footer()}
			{#if draft}
				<Button variant="outline" onclick={() => (draft = null)}>Back</Button>
				<Button type="submit" form="target-profile-form" disabled={saving || error !== null}>Save</Button>
			{:else}
				<Button variant="outline" onclick={() => edit(null)}><Plus /> Add</Button>
				<Button onclick={() => (manageOpen = false)}>Done</Button>
			{/if}
		{/snippet}
	</Modal>
{/if}
