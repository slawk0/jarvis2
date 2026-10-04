<!-- Create or edit a database connection profile (password goes to the keyring). -->
<script lang="ts">
	import Wand from '@lucide/svelte/icons/wand-sparkles';
	import Field from '$lib/components/Field.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, apiQuiet, type DbProfile, type Engine } from '$lib/ipc';
	import { toast } from '$lib/services/toast.svelte';

	interface Props {
		/** The profile being edited; `null` closes the dialog. */
		profile: DbProfile | null;
		onsaved: (profile: DbProfile) => void;
	}

	let { profile = $bindable(), onsaved }: Props = $props();

	const PORTS: Record<Engine, number> = { mysql: 3306, postgres: 5432 };

	let draft = $state<DbProfile | null>(null);
	let password = $state('');
	/** The password field was edited (otherwise the stored one is kept). */
	let passwordTouched = $state(false);
	let containers = $state<string[]>([]);
	let saving = $state(false);
	let detecting = $state(false);

	$effect(() => {
		if (!profile) {
			draft = null;
			return;
		}
		draft = structuredClone($state.snapshot(profile));
		password = '';
		passwordTouched = false;
		apiQuiet.dockerContainers().then(
			(list) => (containers = list.filter((c) => c.state === 'running').map((c) => c.name)),
			() => (containers = [])
		);
	});

	const isNew = $derived(!draft?.id);
	const error = $derived.by(() => {
		if (!draft) return null;
		if (!draft.name.trim()) return 'Enter a name.';
		if (draft.source === 'container' && !draft.container) return 'Choose a container.';
		if (draft.source === 'host' && !draft.host.trim()) return 'Enter the database host.';
		if (!draft.user.trim()) return 'Enter the database user.';
		if (draft.engine === 'postgres' && !draft.database.trim())
			return 'PostgreSQL needs a database to log in to.';
		return null;
	});

	function setEngine(engine: Engine): void {
		if (!draft) return;
		const wasDefault = Object.values(PORTS).includes(draft.port);
		draft.engine = engine;
		if (wasDefault) draft.port = PORTS[engine];
	}

	async function detect(): Promise<void> {
		if (!draft?.container) return;
		detecting = true;
		try {
			const found = await api.dbDetect(draft.container);
			if (found.engine) draft.engine = found.engine;
			draft.port = found.port;
			if (found.user) draft.user = found.user;
			if (found.database) draft.database = found.database;
			if (found.password) {
				password = found.password;
				passwordTouched = true;
			}
			if (!draft.name.trim()) draft.name = draft.container;
			if (found.engine) toast.success('Filled in from the container’s environment');
			else toast.warning('This does not look like a MySQL or PostgreSQL container');
		} catch (e) {
			toast.error(e, 'Could not inspect the container');
		} finally {
			detecting = false;
		}
	}

	async function save(): Promise<void> {
		if (!draft || error) return;
		saving = true;
		try {
			const saved = await api.dbProfileSave(
				$state.snapshot(draft),
				passwordTouched || isNew ? password : null
			);
			profile = null;
			onsaved(saved);
		} catch (e) {
			toast.error(e, 'Could not save the connection');
		} finally {
			saving = false;
		}
	}
</script>

{#if draft}
	<Modal
		open
		title={isNew ? 'New database connection' : 'Edit database connection'}
		description="The database is reached through the SSH connection. The password is stored in your system keyring."
		size="lg"
		onclose={() => (profile = null)}
	>
		<form
			class="grid grid-cols-2 gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void save();
			}}
		>
			<Field label="Name" class="col-span-2">
				<Input bind:value={draft.name} placeholder="e.g. Shop database" autofocus />
			</Field>
			<Field label="Where is the database?">
				<SelectField
					bind:value={draft.source}
					options={[
						{ value: 'host', label: 'Host and port' },
						{ value: 'container', label: 'Docker container' }
					]}
				/>
			</Field>
			<Field label="Engine">
				<SelectField
					value={draft.engine}
					onchange={setEngine}
					options={[
						{ value: 'mysql', label: 'MySQL / MariaDB' },
						{ value: 'postgres', label: 'PostgreSQL' }
					]}
				/>
			</Field>
			{#if draft.source === 'container'}
				<Field label="Container" class="col-span-2" hint="Jarvis connects to the container’s own IP address.">
					<div class="flex gap-2">
						<SelectField
							bind:value={draft.container}
							options={containers}
							placeholder={containers.length ? 'Choose a container…' : 'No running containers'}
						/>
						<Button type="button" variant="outline" disabled={!draft.container || detecting} onclick={detect}>
							<Wand />
							{detecting ? 'Detecting…' : 'Auto-detect'}
						</Button>
					</div>
				</Field>
			{:else}
				<Field label="Host" hint="As seen from the server, e.g. 127.0.0.1">
					<Input bind:value={draft.host} class="font-mono text-xs" spellcheck="false" />
				</Field>
			{/if}
			<Field label="Port" class={draft.source === 'container' ? 'col-span-2' : ''}>
				<Input type="number" min="1" max="65535" bind:value={draft.port} />
			</Field>
			<Field label="User">
				<Input bind:value={draft.user} spellcheck="false" autocomplete="off" />
			</Field>
			<Field label="Password" hint={isNew ? undefined : 'Leave empty to keep the stored password.'}>
				<Input
					type="password"
					bind:value={password}
					oninput={() => (passwordTouched = true)}
					autocomplete="new-password"
				/>
			</Field>
			<Field
				label="Default database"
				class="col-span-2"
				hint={draft.engine === 'postgres' ? 'Required for PostgreSQL.' : 'Optional.'}
			>
				<Input bind:value={draft.database} spellcheck="false" />
			</Field>
			<button type="submit" class="hidden" aria-hidden="true" tabindex="-1"></button>
		</form>
		{#snippet footer()}
			{#if error}<p class="mr-auto text-xs text-muted-foreground">{error}</p>{/if}
			<Button variant="outline" onclick={() => (profile = null)}>Cancel</Button>
			<Button disabled={!!error || saving} onclick={save}>{saving ? 'Saving…' : 'Save'}</Button>
		{/snippet}
	</Modal>
{/if}
