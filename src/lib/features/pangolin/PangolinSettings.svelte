<!-- Connection to the Pangolin Integration API: URL, API key (write-only) and organisation. -->
<script lang="ts">
	import { openUrl } from '@tauri-apps/plugin-opener';
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import Field from '$lib/components/Field.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, toIpcError, type IpcError, type PangolinOrg, type PangolinStatus } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { settings } from '$lib/services/settings.svelte';
	import { toast } from '$lib/services/toast.svelte';

	interface Props {
		status: PangolinStatus | null;
		/** Called after the configuration changed. */
		onchange: () => void;
	}

	let { status, onchange }: Props = $props();

	const DEFAULT_URL = 'https://api.pangolin.net';
	const DOCS = 'https://docs.pangolin.net/manage/integration-api';

	// svelte-ignore state_referenced_locally
	let apiUrl = $state(status?.apiUrl || DEFAULT_URL);
	let apiKey = $state('');
	// svelte-ignore state_referenced_locally
	let orgId = $state(status?.orgId ?? '');
	let orgs = $state<PangolinOrg[]>([]);
	let saving = $state(false);
	let result = $state<{ kind: 'ok' | 'limited'; message: string } | null>(null);
	let error = $state<IpcError | null>(null);

	const hasKey = $derived(status?.configured ?? false);

	async function save(): Promise<void> {
		saving = true;
		error = null;
		result = null;
		try {
			const verify = await api.pangolinConfigure(apiUrl.trim(), apiKey ? apiKey : null, orgId.trim());
			orgs = verify.orgs;
			apiKey = '';
			await settings.load();
			if (verify.limited) {
				result = {
					kind: 'limited',
					message: orgId.trim()
						? 'The key works. It is scoped to one organisation, so organisations cannot be listed.'
						: 'The key works but cannot list organisations. Enter the organisation ID it belongs to and save again.'
				};
			} else if (!orgId.trim() && verify.orgs.length === 1) {
				orgId = verify.orgs[0].id;
				await api.pangolinConfigure(apiUrl.trim(), null, orgId);
				await settings.load();
				result = { kind: 'ok', message: `Connected. Using organisation “${verify.orgs[0].name}”.` };
			} else if (!orgId.trim()) {
				result = { kind: 'limited', message: 'Connected. Choose an organisation and save again.' };
			} else {
				result = { kind: 'ok', message: 'Connected and verified.' };
			}
			onchange();
		} catch (raw) {
			error = toIpcError(raw);
		} finally {
			saving = false;
		}
	}

	async function disconnect(): Promise<void> {
		const ok = await confirm({
			title: 'Remove the Pangolin connection?',
			message: 'The API key is deleted from your system keyring.',
			confirmLabel: 'Remove',
			destructive: true
		});
		if (!ok) return;
		try {
			await api.pangolinClear();
			await settings.load();
			apiKey = '';
			orgId = '';
			orgs = [];
			result = null;
			toast.success('Pangolin connection removed');
			onchange();
		} catch (e) {
			toast.error(e);
		}
	}
</script>

<div class="mx-auto flex w-full max-w-xl flex-col gap-4 overflow-y-auto py-2">
	<div>
		<h3 class="text-base font-semibold">Pangolin Integration API</h3>
		<p class="text-sm text-muted-foreground">
			Jarvis talks to Pangolin over its HTTP API, not over SSH. Create an API key in Pangolin under Settings →
			API Keys.
			<button
				type="button"
				class="inline-flex items-center gap-1 text-primary hover:underline"
				onclick={() => openUrl(DOCS)}>Documentation <ExternalLink class="size-3" /></button
			>
		</p>
	</div>
	<form
		class="flex flex-col gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void save();
		}}
	>
		<Field
			label="API URL"
			hint="For a self-hosted Pangolin this is usually https://api.your-domain. Jarvis checks both /v1 and /api/v1."
		>
			<Input bind:value={apiUrl} placeholder={DEFAULT_URL} spellcheck="false" class="font-mono text-xs" />
		</Field>
		<Field
			label="API key"
			hint={hasKey ? 'A key is stored. Leave empty to keep it.' : 'Stored only in your system keyring.'}
		>
			<Input
				type="password"
				bind:value={apiKey}
				placeholder={hasKey ? '••••••••  (stored)' : 'keyId.secret'}
				autocomplete="new-password"
			/>
		</Field>
		<Field
			label="Organisation"
			hint="Organisation-scoped keys cannot list organisations: type the organisation ID."
		>
			{#if orgs.length > 1}
				<SelectField
					bind:value={orgId}
					options={orgs.map((o) => ({ value: o.id, label: `${o.name} (${o.id})` }))}
					placeholder="Choose an organisation…"
				/>
			{:else}
				<Input bind:value={orgId} placeholder="Organisation ID" spellcheck="false" />
			{/if}
		</Field>
		<div class="flex items-center gap-2">
			<Button type="submit" disabled={saving || !apiUrl.trim() || (!hasKey && !apiKey)}
				>{saving ? 'Verifying…' : 'Save & verify'}</Button
			>
			{#if hasKey}<Button type="button" variant="outline" onclick={disconnect}>Remove connection</Button>{/if}
		</div>
	</form>
	{#if error}
		<p
			class="selectable flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm"
			role="alert"
		>
			<TriangleAlert class="mt-0.5 size-4 shrink-0 text-destructive" />
			{error.message}
		</p>
	{:else if result}
		<p
			class={result.kind === 'ok'
				? 'flex items-start gap-2 rounded-md border border-success/40 bg-success/10 px-3 py-2 text-sm'
				: 'flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm'}
			role="status"
		>
			{#if result.kind === 'ok'}<CircleCheck
					class="mt-0.5 size-4 shrink-0 text-success"
				/>{:else}<TriangleAlert class="mt-0.5 size-4 shrink-0 text-warning" />{/if}
			{result.message}
		</p>
	{/if}
</div>
