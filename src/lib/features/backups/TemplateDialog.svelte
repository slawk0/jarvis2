<!-- Create or edit a backup template: what to back up, where to, when, and how long to keep it. -->
<script lang="ts" module>
	import type { BackupTemplate } from '$lib/ipc';

	export const blankTemplate = (): BackupTemplate => ({
		id: '',
		name: '',
		kind: 'files',
		path: '',
		dbSource: 'host',
		dbHost: '127.0.0.1',
		dbPort: 3306,
		dbContainer: '',
		dbName: '',
		dbUser: '',
		destination: 'folder',
		folder: '/var/backups',
		s3Endpoint: '',
		s3Region: '',
		s3Bucket: '',
		s3Prefix: '',
		sftpHost: '',
		sftpPort: 22,
		sftpUser: '',
		sftpPath: '',
		resticRepo: '',
		schedule: '',
		paused: false,
		sudo: false,
		keepDays: 0,
		keep: { last: 0, daily: 0, weekly: 0, monthly: 0 }
	});

	export const KIND_LABEL: Record<string, string> = {
		files: 'Files',
		mysql: 'MySQL',
		postgres: 'PostgreSQL'
	};
	export const DESTINATION_LABEL: Record<string, string> = {
		download: 'Download',
		folder: 'Server folder',
		s3: 'S3',
		sftp: 'SFTP',
		restic: 'Restic'
	};
</script>

<script lang="ts">
	import Wand from '@lucide/svelte/icons/wand-sparkles';
	import CronInput from '$lib/components/CronInput.svelte';
	import Field from '$lib/components/Field.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import PathInput from '$lib/components/PathInput.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { isValidCron } from '$lib/cron';
	import { api, apiQuiet, type ResticRepo } from '$lib/ipc';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';

	interface Props {
		template: BackupTemplate | null;
		onsaved: (template: BackupTemplate) => void;
	}

	let { template = $bindable(), onsaved }: Props = $props();

	let draft = $state<BackupTemplate | null>(null);
	let scheduled = $state(false);
	let schedule = $state('0 3 * * *');
	let dbPassword = $state('');
	let s3AccessKey = $state('');
	let s3SecretKey = $state('');
	let sftpPassword = $state('');
	/** Secret fields the user typed into (others keep their stored value). */
	let touched = $state(new Set<string>());
	let containers = $state<string[]>([]);
	let repos = $state<ResticRepo[]>([]);
	let detecting = $state(false);
	let saving = $state(false);

	$effect(() => {
		if (!template) {
			draft = null;
			return;
		}
		// Read from a local: reading `draft` here would make the effect depend on what it writes.
		const next = structuredClone($state.snapshot(template));
		draft = next;
		scheduled = next.schedule.trim() !== '';
		schedule = next.schedule.trim() || '0 3 * * *';
		dbPassword = s3AccessKey = s3SecretKey = sftpPassword = '';
		touched = new Set();
		apiQuiet.dockerContainers().then(
			(list) => (containers = list.filter((c) => c.state === 'running').map((c) => c.name)),
			() => (containers = [])
		);
		apiQuiet.resticRepos().then(
			(list) => (repos = list),
			() => (repos = [])
		);
	});

	const isNew = $derived(!draft?.id);
	const isDb = $derived(draft ? draft.kind !== 'files' : false);
	const canSchedule = $derived(draft?.destination !== 'download');
	const keptHint = $derived(isNew ? undefined : 'Empty keeps the stored value.');

	const error = $derived.by(() => {
		const d = draft;
		if (!d) return null;
		if (!d.name.trim()) return 'Enter a name.';
		if (!isDb) {
			if (!d.path.trim().startsWith('/') || d.path.trim().replace(/\/+$/, '') === '')
				return 'Enter the absolute path of the file or folder to back up.';
		} else {
			if (d.dbSource === 'container' ? !d.dbContainer : !d.dbHost.trim())
				return 'Choose where the database runs.';
			if (!d.dbName.trim() || !d.dbUser.trim()) return 'Enter the database name and user.';
		}
		if (d.destination === 'folder' && !d.folder.trim().startsWith('/'))
			return 'Enter the absolute path of the destination folder.';
		if (d.destination === 's3' && !d.s3Bucket.trim()) return 'Enter the bucket name.';
		if (d.destination === 'sftp' && (!d.sftpHost.trim() || !d.sftpUser.trim() || !d.sftpPath.trim()))
			return 'Enter the SFTP host, user and path.';
		if (d.destination === 'restic' && !d.resticRepo)
			return repos.length
				? 'Choose a restic repository.'
				: 'Add a repository in the Restic Backups tab first.';
		if (scheduled && canSchedule && !isValidCron(schedule))
			return 'The schedule is not a valid cron expression.';
		return null;
	});

	function setKind(kind: string): void {
		if (!draft) return;
		draft.kind = kind;
		if (kind === 'mysql' && draft.dbPort === 5432) draft.dbPort = 3306;
		if (kind === 'postgres' && draft.dbPort === 3306) draft.dbPort = 5432;
	}

	function touch(name: string): void {
		touched = new Set([...touched, name]);
	}

	async function detect(): Promise<void> {
		if (!draft?.dbContainer) return;
		detecting = true;
		try {
			const found = await api.dbDetect(draft.dbContainer);
			if (found.engine) draft.kind = found.engine;
			draft.dbPort = found.port;
			if (found.user) draft.dbUser = found.user;
			if (found.database) draft.dbName = found.database;
			if (found.password) {
				dbPassword = found.password;
				touch('db');
			}
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
			const next = $state.snapshot(draft);
			next.schedule = scheduled && canSchedule ? schedule.trim() : '';
			if (!next.schedule) next.paused = false;
			const secret = (name: string, value: string) => (touched.has(name) || isNew ? value : null);
			const saved = await sudo.describe('Install the backup schedule', () =>
				api.backupTemplateSave(next, {
					dbPassword: secret('db', dbPassword),
					s3AccessKey: secret('s3a', s3AccessKey),
					s3SecretKey: secret('s3s', s3SecretKey),
					sftpPassword: secret('sftp', sftpPassword)
				})
			);
			template = null;
			onsaved(saved);
		} catch (e) {
			toast.error(e, 'Could not save the backup');
		} finally {
			saving = false;
		}
	}
</script>

{#if draft}
	<Modal
		open
		title={isNew ? 'New backup' : 'Edit backup'}
		description="Passwords and keys are stored in your system keyring. A schedule also keeps them in a root-only file on the server."
		size="xl"
		onclose={() => (template = null)}
	>
		<div class="grid grid-cols-2 gap-x-4 gap-y-3">
			<Field label="Name" class="col-span-2">
				<Input bind:value={draft.name} placeholder="e.g. Website files" autofocus />
			</Field>

			<h3 class="col-span-2 mt-1 text-sm font-semibold">What to back up</h3>
			<Field label="Type" class={isDb ? '' : 'col-span-2'}>
				<SelectField
					value={draft.kind}
					onchange={setKind}
					options={[
						{ value: 'files', label: 'Files or a folder' },
						{ value: 'mysql', label: 'MySQL / MariaDB database' },
						{ value: 'postgres', label: 'PostgreSQL database' }
					]}
				/>
			</Field>
			{#if !isDb}
				<Field
					label="Path"
					class="col-span-2"
					hint="A file or folder on the server. It is packed into a .tar.gz archive."
				>
					<PathInput bind:value={draft.path} />
				</Field>
			{:else}
				<Field label="Where is the database?">
					<SelectField
						bind:value={draft.dbSource}
						options={[
							{ value: 'host', label: 'Host and port' },
							{ value: 'container', label: 'Docker container' }
						]}
					/>
				</Field>
				{#if draft.dbSource === 'container'}
					<Field
						label="Container"
						class="col-span-2"
						hint="The dump runs inside the container, so no client is needed on the server."
					>
						<div class="flex gap-2">
							<SelectField
								bind:value={draft.dbContainer}
								options={containers}
								placeholder={containers.length ? 'Choose a container…' : 'No running containers'}
							/>
							<Button
								type="button"
								variant="outline"
								disabled={!draft.dbContainer || detecting}
								onclick={detect}><Wand /> {detecting ? 'Detecting…' : 'Auto-detect'}</Button
							>
						</div>
					</Field>
				{:else}
					<Field label="Host"
						><Input bind:value={draft.dbHost} class="font-mono text-xs" spellcheck="false" /></Field
					>
					<Field label="Port"><Input type="number" min="1" max="65535" bind:value={draft.dbPort} /></Field>
				{/if}
				<Field label="Database"><Input bind:value={draft.dbName} spellcheck="false" /></Field>
				<Field label="User"><Input bind:value={draft.dbUser} spellcheck="false" autocomplete="off" /></Field>
				<Field label="Password" class="col-span-2" hint={keptHint}>
					<Input
						type="password"
						bind:value={dbPassword}
						oninput={() => touch('db')}
						autocomplete="new-password"
					/>
				</Field>
			{/if}

			<h3 class="col-span-2 mt-1 text-sm font-semibold">Where to store it</h3>
			<Field label="Destination" class="col-span-2">
				<SelectField
					bind:value={draft.destination}
					options={[
						{ value: 'folder', label: 'A folder on the server' },
						{ value: 'download', label: 'Download to this computer' },
						{ value: 's3', label: 'S3-compatible storage (also Backblaze B2)' },
						{ value: 'sftp', label: 'Another server over SFTP' },
						{ value: 'restic', label: 'A restic repository' }
					]}
				/>
			</Field>
			{#if draft.destination === 'folder'}
				<Field label="Folder" class="col-span-2"><PathInput bind:value={draft.folder} dirsOnly /></Field>
			{:else if draft.destination === 'download'}
				<p class="col-span-2 text-xs text-muted-foreground">
					You choose the folder each time you run the backup. Downloads cannot be scheduled.
				</p>
			{:else if draft.destination === 's3'}
				<Field label="Endpoint" hint="Leave empty for Amazon S3."
					><Input
						bind:value={draft.s3Endpoint}
						placeholder="https://s3.eu-central-003.backblazeb2.com"
						spellcheck="false"
					/></Field
				>
				<Field label="Region" hint="Optional."
					><Input bind:value={draft.s3Region} placeholder="eu-central-1" spellcheck="false" /></Field
				>
				<Field label="Bucket"><Input bind:value={draft.s3Bucket} spellcheck="false" /></Field>
				<Field label="Prefix" hint="Optional folder inside the bucket."
					><Input bind:value={draft.s3Prefix} spellcheck="false" /></Field
				>
				<Field label="Access key" hint={keptHint}
					><Input
						bind:value={s3AccessKey}
						oninput={() => touch('s3a')}
						spellcheck="false"
						autocomplete="off"
					/></Field
				>
				<Field label="Secret key" hint={keptHint}
					><Input
						type="password"
						bind:value={s3SecretKey}
						oninput={() => touch('s3s')}
						autocomplete="new-password"
					/></Field
				>
			{:else if draft.destination === 'sftp'}
				<Field label="Host"><Input bind:value={draft.sftpHost} spellcheck="false" /></Field>
				<Field label="Port"><Input type="number" min="1" max="65535" bind:value={draft.sftpPort} /></Field>
				<Field label="User"><Input bind:value={draft.sftpUser} spellcheck="false" autocomplete="off" /></Field
				>
				<Field label="Password" hint={keptHint}
					><Input
						type="password"
						bind:value={sftpPassword}
						oninput={() => touch('sftp')}
						autocomplete="new-password"
					/></Field
				>
				<Field label="Remote path" class="col-span-2"
					><Input bind:value={draft.sftpPath} placeholder="/backups/web" spellcheck="false" /></Field
				>
			{:else if draft.destination === 'restic'}
				<Field
					label="Repository"
					class="col-span-2"
					hint={isDb
						? 'The dump is stored as one file in a new snapshot.'
						: 'The path is backed up directly (no archive), so restic can deduplicate it.'}
				>
					<SelectField
						bind:value={draft.resticRepo}
						options={repos.map((r) => ({ value: r.id, label: r.name }))}
						placeholder={repos.length ? 'Choose a repository…' : 'No repositories configured'}
					/>
				</Field>
			{/if}

			<h3 class="col-span-2 mt-1 text-sm font-semibold">Retention</h3>
			{#if draft.destination === 'restic'}
				<div class="col-span-2 grid grid-cols-4 gap-3">
					<Field label="Keep last"><Input type="number" min="0" bind:value={draft.keep.last} /></Field>
					<Field label="Keep daily"><Input type="number" min="0" bind:value={draft.keep.daily} /></Field>
					<Field label="Keep weekly"><Input type="number" min="0" bind:value={draft.keep.weekly} /></Field>
					<Field label="Keep monthly"><Input type="number" min="0" bind:value={draft.keep.monthly} /></Field>
				</div>
				<p class="col-span-2 -mt-1 text-xs text-muted-foreground">
					Applies only to this backup’s snapshots. All zero keeps every snapshot.
				</p>
			{:else if draft.destination === 'download'}
				<p class="col-span-2 text-xs text-muted-foreground">Downloaded archives are yours to manage.</p>
			{:else}
				<Field
					label="Delete archives older than (days)"
					class="col-span-2"
					hint="0 keeps every archive. Only this backup’s own archives are deleted."
				>
					<Input type="number" min="0" bind:value={draft.keepDays} class="w-40" />
				</Field>
			{/if}

			<h3 class="col-span-2 mt-1 text-sm font-semibold">Schedule</h3>
			{#if canSchedule}
				<label class="col-span-2 flex items-center gap-2 text-sm">
					<Checkbox bind:checked={scheduled} /> Run automatically (installed in root’s crontab on the server)
				</label>
				{#if scheduled}
					<div class="col-span-2"><CronInput bind:value={schedule} /></div>
				{/if}
			{:else}
				<p class="col-span-2 text-xs text-muted-foreground">Not available for downloads.</p>
			{/if}
			<label class="col-span-2 flex items-center gap-2 text-sm">
				<Checkbox bind:checked={draft.sudo} /> Run as root (needed for files only root can read; scheduled archives
				always run as root)
			</label>
		</div>
		{#snippet footer()}
			{#if error}<p class="mr-auto text-xs text-muted-foreground">{error}</p>{/if}
			<Button variant="outline" onclick={() => (template = null)}>Cancel</Button>
			<Button disabled={!!error || saving} onclick={save}>{saving ? 'Saving…' : 'Save'}</Button>
		{/snippet}
	</Modal>
{/if}
