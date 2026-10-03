<!-- App settings: appearance, terminal, downloads, alerts, known hosts, updates, about. -->
<script lang="ts">
	import { getVersion } from '@tauri-apps/api/app';
	import { open as pickFolder } from '@tauri-apps/plugin-dialog';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import SubTabs from '$lib/components/SubTabs.svelte';
	import { TERMINAL_THEMES } from '$lib/features/terminal/themes';
	import { api, type KnownHost, type Theme } from '$lib/ipc';
	import { app } from '$lib/services/app.svelte';
	import { confirm } from '$lib/services/confirm.svelte';
	import { loadDoc, saveDoc, type AlertThresholds } from '$lib/services/profile-data';
	import { settings } from '$lib/services/settings.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { updater } from '$lib/services/updater.svelte';
	import { clamp } from '$lib/utils';
	import { workspace } from '$lib/workspace/workspace.svelte';

	type Section = 'general' | 'terminal' | 'alerts' | 'hosts' | 'about';
	let section = $state<Section>('general');
	let version = $state('');
	let downloadDir = $state('');
	let knownHosts = $state<KnownHost[]>([]);
	let thresholds = $state<AlertThresholds>({ enabled: false, cpu: 95, ram: 90, disk: 85 });

	const sections = $derived([
		{ id: 'general' as const, label: 'General' },
		{ id: 'terminal' as const, label: 'Terminal' },
		...(app.session ? [{ id: 'alerts' as const, label: 'Alerts' }] : []),
		{ id: 'hosts' as const, label: 'Known hosts' },
		{ id: 'about' as const, label: 'About' }
	]);

	$effect(() => {
		if (!workspace.settingsOpen) return;
		void load();
	});

	async function load(): Promise<void> {
		try {
			version = await getVersion();
			downloadDir = await api.defaultDownloadDir();
			knownHosts = await api.knownHostsList();
			if (app.session) {
				thresholds = await loadDoc('alertThresholds', { enabled: false, cpu: 95, ram: 90, disk: 85 });
			}
		} catch (error) {
			toast.error(error);
		}
	}

	async function chooseDownloadDir(): Promise<void> {
		const picked = await pickFolder({ directory: true, defaultPath: downloadDir });
		if (typeof picked !== 'string') return;
		settings.update((s) => (s.downloadDir = picked));
		downloadDir = picked;
	}

	async function forget(host: KnownHost): Promise<void> {
		const ok = await confirm({
			title: `Forget ${host.host}?`,
			message: 'The next connection to this server will ask you to verify its fingerprint again.',
			confirmLabel: 'Forget host',
			destructive: true
		});
		if (!ok) return;
		try {
			await api.knownHostForget(host.host, host.port);
			knownHosts = await api.knownHostsList();
		} catch (error) {
			toast.error(error);
		}
	}

	async function saveThresholds(): Promise<void> {
		thresholds.cpu = clamp(Math.round(thresholds.cpu) || 95, 1, 100);
		thresholds.ram = clamp(Math.round(thresholds.ram) || 90, 1, 100);
		thresholds.disk = clamp(Math.round(thresholds.disk) || 85, 1, 100);
		try {
			await saveDoc('alertThresholds', $state.snapshot(thresholds));
		} catch (error) {
			toast.error(error, 'Could not save alert settings');
		}
	}

	const THEMES: { value: Theme; label: string }[] = [
		{ value: 'dark', label: 'Dark' },
		{ value: 'light', label: 'Light' },
		{ value: 'system', label: 'Follow system' }
	];
	const FONTS = ['JetBrains Mono Variable', 'Cascadia Code', 'Consolas', 'Menlo', 'DejaVu Sans Mono', 'monospace'];
</script>

<Modal bind:open={workspace.settingsOpen} title="Settings" size="lg" class="h-[34rem]">
	<SubTabs items={sections} bind:value={section} class="mb-4" />

	{#if section === 'general'}
		<div class="flex flex-col gap-4">
			<Field label="Theme">
				<SelectField
					value={settings.value.theme}
					options={THEMES}
					class="w-56"
					onchange={(v) => settings.update((s) => (s.theme = v))}
				/>
			</Field>
			<Field label="Default download folder" hint="Where files and backups downloaded from servers are saved.">
				<div class="flex gap-2">
					<Input value={downloadDir} readonly class="font-mono text-xs" />
					<Button variant="outline" onclick={chooseDownloadDir}>Choose…</Button>
				</div>
			</Field>
		</div>
	{:else if section === 'terminal'}
		<div class="grid grid-cols-2 gap-4">
			<Field label="Font">
				<SelectField
					value={settings.value.terminal.fontFamily}
					options={FONTS}
					onchange={(v) => settings.update((s) => (s.terminal.fontFamily = v))}
				/>
			</Field>
			<Field label="Font size">
				<Input
					type="number"
					min="8"
					max="32"
					value={settings.value.terminal.fontSize}
					onchange={(e) => {
						const size = clamp(Number(e.currentTarget.value) || 13, 8, 32);
						settings.update((s) => (s.terminal.fontSize = size));
					}}
				/>
			</Field>
			<Field label="Colour theme">
				<SelectField
					value={settings.value.terminal.theme}
					options={Object.entries(TERMINAL_THEMES).map(([value, t]) => ({ value, label: t.label }))}
					onchange={(v) => settings.update((s) => (s.terminal.theme = v))}
				/>
			</Field>
			<Field label="Scrollback lines">
				<Input
					type="number"
					min="500"
					max="100000"
					step="500"
					value={settings.value.terminal.scrollback}
					onchange={(e) => {
						const lines = clamp(Number(e.currentTarget.value) || 10000, 500, 100000);
						settings.update((s) => (s.terminal.scrollback = lines));
					}}
				/>
			</Field>
			<label class="col-span-2 flex items-center justify-between gap-4 text-sm">
				<span>
					Copy on select
					<span class="text-muted-foreground block text-xs">Selecting text copies it to the clipboard.</span>
				</span>
				<Switch
					checked={settings.value.terminal.copyOnSelect}
					onCheckedChange={(v) => settings.update((s) => (s.terminal.copyOnSelect = v))}
				/>
			</label>
			<label class="col-span-2 flex items-center justify-between gap-4 text-sm">
				<span>
					Right-click pastes
					<span class="text-muted-foreground block text-xs">Shift+right-click always opens the menu.</span>
				</span>
				<Switch
					checked={settings.value.terminal.rightClickPaste}
					onCheckedChange={(v) => settings.update((s) => (s.terminal.rightClickPaste = v))}
				/>
			</label>
		</div>
	{:else if section === 'alerts'}
		<div class="flex flex-col gap-4">
			<label class="flex items-center justify-between gap-4 text-sm">
				<span>
					Desktop alerts for {app.profile?.label}
					<span class="text-muted-foreground block text-xs">
						Notify when a resource stays above its threshold (at most every 5 minutes per metric).
					</span>
				</span>
				<Switch bind:checked={thresholds.enabled} onCheckedChange={saveThresholds} />
			</label>
			<div class="grid grid-cols-3 gap-3">
				<Field label="CPU %">
					<Input type="number" min="1" max="100" bind:value={thresholds.cpu} onchange={saveThresholds} disabled={!thresholds.enabled} />
				</Field>
				<Field label="RAM %">
					<Input type="number" min="1" max="100" bind:value={thresholds.ram} onchange={saveThresholds} disabled={!thresholds.enabled} />
				</Field>
				<Field label="Disk %">
					<Input type="number" min="1" max="100" bind:value={thresholds.disk} onchange={saveThresholds} disabled={!thresholds.enabled} />
				</Field>
			</div>
		</div>
	{:else if section === 'hosts'}
		{#if knownHosts.length === 0}
			<p class="text-muted-foreground text-sm">No trusted hosts yet. They are added when you first connect to a server.</p>
		{:else}
			<ul class="flex flex-col divide-y rounded-lg border">
				{#each knownHosts as host (host.host + host.port + host.fingerprint)}
					<li class="flex items-center gap-3 px-3 py-2">
						<div class="selectable min-w-0 flex-1">
							<p class="truncate text-sm font-medium">
								{host.host}{host.port === 22 ? '' : `:${host.port}`}
								<span class="text-muted-foreground text-xs font-normal">{host.keyType}</span>
							</p>
							<p class="text-muted-foreground truncate font-mono text-[11px]">{host.fingerprint}</p>
						</div>
						<IconButton label="Forget host" onclick={() => forget(host)}><Trash2 /></IconButton>
					</li>
				{/each}
			</ul>
		{/if}
	{:else}
		<div class="flex flex-col gap-4">
			<div class="flex items-center gap-3">
				<img src="/logo.svg" alt="" class="size-12" />
				<div>
					<p class="font-semibold">Jarvis Server Manager</p>
					<p class="text-muted-foreground selectable text-sm">Version {version}</p>
				</div>
			</div>
			<p class="text-muted-foreground text-sm">
				Manage Linux servers over SSH. Nothing is installed on the server; credentials stay in your system keyring.
			</p>
			<div>
				<Button variant="outline" disabled={updater.phase === 'checking'} onclick={() => updater.check(false)}>
					{updater.phase === 'checking' ? 'Checking…' : 'Check for updates'}
				</Button>
			</div>
		</div>
	{/if}
</Modal>
