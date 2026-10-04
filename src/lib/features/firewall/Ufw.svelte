<script lang="ts">
	import Plus from '@lucide/svelte/icons/plus';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import ShieldOff from '@lucide/svelte/icons/shield-off';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { api, type UfwRule } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { autoLoad, Busy, resource } from '$lib/state/resource.svelte';
	import { cn } from '$lib/utils';

	let { visible, sshPort }: { visible: boolean; sshPort: number } = $props();

	const status = resource((io) => io.ufwStatus());
	const busy = new Busy();
	let addOpen = $state(false);
	let form = $state({ action: 'allow', port: '', protocol: 'tcp', source: '' });
	let enableOpen = $state(false);
	let allowSsh = $state(true);
	let working = $state(false);

	autoLoad(status, () => visible);

	const columns: Column<UfwRule>[] = [
		{ key: 'number', label: 'No.', value: (r) => r.number, align: 'right', class: 'w-16 tabular' },
		{ key: 'to', label: 'To / port', value: (r) => r.to, mono: true, class: 'w-72 max-w-72' },
		{ key: 'action', label: 'Action', value: (r) => r.action, class: 'w-36' },
		{ key: 'from', label: 'From', value: (r) => r.from, mono: true, class: 'max-w-0 w-full' }
	];

	const sshAllowed = $derived(
		(status.data?.rules ?? []).some(
			(r) =>
				r.action.startsWith('ALLOW') &&
				(r.to.startsWith(`${sshPort}/`) ||
					r.to === String(sshPort) ||
					r.to.startsWith(`${sshPort} `) ||
					r.to.toLowerCase().includes('openssh'))
		)
	);

	async function setEnabled(enable: boolean, allowPort: number | null): Promise<void> {
		working = true;
		try {
			await sudo.describe(enable ? 'Enable the firewall' : 'Disable the firewall', () =>
				api.ufwSetEnabled(enable, allowPort)
			);
			toast.success(enable ? 'Firewall enabled' : 'Firewall disabled');
			enableOpen = false;
		} catch (error) {
			toast.error(error);
		} finally {
			working = false;
			await status.refresh();
		}
	}

	async function disable(): Promise<void> {
		const ok = await confirm({
			title: 'Disable the firewall?',
			message: 'All ports become reachable from the network until it is enabled again.',
			confirmLabel: 'Disable firewall',
			destructive: true
		});
		if (ok) await setEnabled(false, null);
	}

	async function add(): Promise<void> {
		try {
			await sudo.describe('Add a firewall rule', () => api.ufwAddRule($state.snapshot(form)));
			toast.success('Rule added');
			addOpen = false;
			await status.refresh();
		} catch (error) {
			toast.error(error, 'Could not add the rule');
		}
	}

	async function remove(rule: UfwRule): Promise<void> {
		const ok = await confirm({
			title: `Delete rule ${rule.number}?`,
			detail: `${rule.to}  ${rule.action}  ${rule.from}`,
			message: rule.to.startsWith(`${sshPort}/`)
				? 'This looks like the rule that allows your SSH connection. Deleting it can lock you out.'
				: undefined,
			confirmLabel: 'Delete rule',
			destructive: true
		});
		if (!ok) return;
		await busy.run(
			String(rule.number),
			() => sudo.describe('Delete a firewall rule', () => api.ufwDeleteRule(rule.number)),
			(e) => toast.error(e)
		);
		// Rule numbers shift after a delete, so always re-read the list.
		await status.refresh();
	}

	export const refresh = () => status.refresh();
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3">
	<div
		class={cn(
			'flex items-center gap-3 rounded-xl border p-3',
			status.data?.active ? 'border-success/40 bg-success/8' : 'border-warning/40 bg-warning/8'
		)}
	>
		{#if status.data?.active}
			<ShieldCheck class="size-5 text-success" />
			<div class="mr-auto">
				<p class="text-sm font-medium">The firewall is active</p>
				<p class="text-xs text-muted-foreground">Incoming connections are filtered by the rules below.</p>
			</div>
			<Button variant="outline" size="sm" disabled={working} onclick={disable}>Disable</Button>
		{:else}
			<ShieldOff class="size-5 text-warning" />
			<div class="mr-auto">
				<p class="text-sm font-medium">{status.data ? 'The firewall is inactive' : 'Firewall status'}</p>
				<p class="text-xs text-muted-foreground">
					{status.data ? 'Rules are stored but not enforced.' : 'Loading…'}
				</p>
			</div>
			<Button
				size="sm"
				disabled={working || !status.data}
				onclick={() => {
					allowSsh = !sshAllowed;
					enableOpen = true;
				}}>Enable</Button
			>
		{/if}
		<Button
			variant="outline"
			size="sm"
			onclick={() => {
				form = { action: 'allow', port: '', protocol: 'tcp', source: '' };
				addOpen = true;
			}}><Plus /> Add rule</Button
		>
		<RefreshControl onrefresh={status.refresh} loading={status.loading} />
	</div>

	<DataTable
		rows={status.data?.rules ?? []}
		{columns}
		rowKey={(r) => `${r.number}|${r.to}|${r.from}`}
		busy={busy.keys}
		loading={status.loading}
		error={status.error}
		onretry={status.refresh}
		empty="No rules"
		emptyHint="Add a rule to allow or block traffic."
		class="flex-1"
	>
		{#snippet cell(rule, column)}
			{#if column.key === 'action'}
				<span class={rule.action.startsWith('ALLOW') ? 'text-success' : 'text-destructive'}
					>{rule.action}</span
				>
			{:else}
				{column.value?.(rule)}
			{/if}
		{/snippet}
		{#snippet actions(rule)}
			<IconButton label="Delete rule" onclick={() => remove(rule)}><Trash2 /></IconButton>
		{/snippet}
	</DataTable>
</div>

<Modal bind:open={addOpen} title="Add UFW rule" size="md">
	<form
		id="ufw-form"
		class="grid grid-cols-2 gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void add();
		}}
	>
		<Field label="Action">
			<SelectField
				bind:value={form.action}
				options={[
					{ value: 'allow', label: 'Allow' },
					{ value: 'deny', label: 'Deny (drop silently)' },
					{ value: 'reject', label: 'Reject (answer with an error)' }
				]}
			/>
		</Field>
		<Field label="Protocol">
			<SelectField
				bind:value={form.protocol}
				options={[
					{ value: 'any', label: 'Any' },
					{ value: 'tcp', label: 'TCP' },
					{ value: 'udp', label: 'UDP' }
				]}
			/>
		</Field>
		<Field label="Port or range" hint="e.g. 443 or 8000:8100. Empty means any port."
			><Input bind:value={form.port} class="font-mono" spellcheck="false" autofocus /></Field
		>
		<Field label="Source" hint="IP or CIDR. Empty means anywhere."
			><Input
				bind:value={form.source}
				class="font-mono"
				placeholder="203.0.113.0/24"
				spellcheck="false"
			/></Field
		>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (addOpen = false)}>Cancel</Button>
		<Button type="submit" form="ufw-form" disabled={!form.port.trim() && !form.source.trim()}>Add rule</Button
		>
	{/snippet}
</Modal>

<Modal bind:open={enableOpen} title="Enable the firewall?" size="md">
	<div class="flex flex-col gap-3 text-sm">
		<p>Once enabled, incoming connections that no rule allows are blocked.</p>
		{#if sshAllowed}
			<p class="text-success">SSH (port {sshPort}) is already allowed, so this connection keeps working.</p>
		{:else}
			<p class="rounded-lg border border-warning/40 bg-warning/10 px-3 py-2">
				No rule allows SSH on port {sshPort}. Enabling the firewall without one will cut off this connection
				and lock you out.
			</p>
			<label class="flex items-center gap-2"
				><Checkbox bind:checked={allowSsh} /> Allow SSH (port {sshPort}/tcp) first</label
			>
		{/if}
	</div>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (enableOpen = false)}>Cancel</Button>
		<Button
			variant={sshAllowed || allowSsh ? 'default' : 'destructive'}
			disabled={working}
			onclick={() => setEnabled(true, !sshAllowed && allowSsh ? sshPort : null)}
		>
			{sshAllowed || allowSsh ? 'Enable firewall' : 'Enable anyway'}
		</Button>
	{/snippet}
</Modal>
