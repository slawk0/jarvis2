<script lang="ts">
	import Plus from '@lucide/svelte/icons/plus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import DataTable, { type Column } from '$lib/components/DataTable.svelte';
	import Field from '$lib/components/Field.svelte';
	import IconButton from '$lib/components/IconButton.svelte';
	import LogViewer from '$lib/components/LogViewer.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import RefreshControl from '$lib/components/RefreshControl.svelte';
	import SelectField from '$lib/components/SelectField.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { api, type IptPosition, type IptRule } from '$lib/ipc';
	import { confirm } from '$lib/services/confirm.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { Busy, resource } from '$lib/state/resource.svelte';

	let { visible }: { visible: boolean } = $props();

	let table = $state('filter');
	let chainName = $state('INPUT');
	let rawOpen = $state(false);
	let addOpen = $state(false);
	let form = $state({
		target: 'ACCEPT',
		protocol: 'tcp',
		source: '',
		destination: '',
		port: '',
		position: 'append',
		line: '1'
	});
	const data = resource((io) => io.iptablesList(table));
	const busy = new Busy();

	let lastTable = '';
	$effect(() => {
		if (visible && table !== lastTable) {
			lastTable = table;
			void data.refresh();
		}
	});

	const chains = $derived(data.data?.chains ?? []);
	const chain = $derived(chains.find((c) => c.name === chainName) ?? chains[0]);
	$effect(() => {
		if (chains.length && !chains.some((c) => c.name === chainName)) chainName = chains[0].name;
	});

	const columns: Column<IptRule>[] = [
		{ key: 'number', label: 'No.', value: (r) => r.number, align: 'right', class: 'w-16 tabular' },
		{ key: 'target', label: 'Target', value: (r) => r.target, class: 'w-36' },
		{ key: 'protocol', label: 'Protocol', value: (r) => r.protocol, class: 'w-24' },
		{ key: 'source', label: 'Source', value: (r) => r.source, mono: true, class: 'w-48' },
		{ key: 'destination', label: 'Destination', value: (r) => r.destination, mono: true, class: 'w-48' },
		{ key: 'extra', label: 'Details', value: (r) => r.extra, mono: true, class: 'max-w-0 w-full' }
	];

	async function add(): Promise<void> {
		const position: IptPosition =
			form.position === 'line'
				? { kind: 'line', number: Number(form.line) || 1 }
				: form.position === 'insert'
					? { kind: 'insert' }
					: { kind: 'append' };
		try {
			await sudo.describe('Add an iptables rule', () =>
				api.iptablesAddRule({
					table,
					chain: chain.name,
					target: form.target,
					protocol: form.protocol,
					source: form.source,
					destination: form.destination,
					port: form.port,
					position
				})
			);
			toast.success('Rule added');
			addOpen = false;
			await data.refresh();
		} catch (error) {
			toast.error(error, 'Could not add the rule');
		}
	}

	async function remove(rule: IptRule): Promise<void> {
		const ok = await confirm({
			title: `Delete rule ${rule.number} from ${chain.name}?`,
			detail: `${rule.target}  ${rule.protocol}  ${rule.source} → ${rule.destination}  ${rule.extra}`,
			confirmLabel: 'Delete rule',
			destructive: true
		});
		if (!ok) return;
		await busy.run(
			String(rule.number),
			() =>
				sudo.describe('Delete an iptables rule', () =>
					api.iptablesDeleteRule(table, chain.name, rule.number)
				),
			(e) => toast.error(e)
		);
		await data.refresh();
	}

	async function setPolicy(policy: string): Promise<void> {
		if (policy === chain.policy) return;
		const ok = await confirm({
			title: `Set the ${chain.name} policy to ${policy}?`,
			message:
				policy === 'DROP'
					? 'Every packet in this chain that no rule accepts will be dropped. Without a rule that accepts your SSH connection you will be locked out immediately.'
					: 'Packets that no rule matches will be accepted.',
			acknowledge:
				policy === 'DROP'
					? 'I have a rule that accepts my SSH connection and understand the risk.'
					: undefined,
			confirmLabel: `Set policy to ${policy}`,
			destructive: policy === 'DROP'
		});
		if (!ok) return;
		try {
			await sudo.describe('Change a chain policy', () => api.iptablesSetPolicy(table, chain.name, policy));
			await data.refresh();
		} catch (error) {
			toast.error(error);
		}
	}

	async function persist(): Promise<void> {
		try {
			const where = await sudo.describe('Save the iptables rules', () => api.iptablesPersist());
			toast.success(`Rules saved (${where})`);
		} catch (error) {
			toast.error(error, 'Could not save the rules');
		}
	}

	export const refresh = () => data.refresh();
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3">
	<div class="flex flex-wrap items-center gap-2">
		<SelectField bind:value={table} class="w-32" options={['filter', 'nat', 'mangle', 'raw']} />
		{#if chains.length}
			<SelectField
				bind:value={chainName}
				class="w-56"
				options={chains.map((c) => ({
					value: c.name,
					label: `${c.name}${c.policy ? ` (policy ${c.policy})` : ''}`
				}))}
			/>
		{/if}
		{#if chain?.policy}
			<span class="text-xs text-muted-foreground">Policy:</span>
			<SelectField
				value={chain.policy}
				class="w-28"
				size="sm"
				options={['ACCEPT', 'DROP']}
				onchange={setPolicy}
			/>
		{/if}
		<span class="flex-1"></span>
		<Button variant="outline" size="sm" onclick={() => (rawOpen = true)}>Raw output</Button>
		<Button variant="outline" size="sm" onclick={persist}>Persist rules</Button>
		<Button
			size="sm"
			disabled={!chain}
			onclick={() => {
				form = {
					target: 'ACCEPT',
					protocol: 'tcp',
					source: '',
					destination: '',
					port: '',
					position: 'append',
					line: '1'
				};
				addOpen = true;
			}}><Plus /> Add rule</Button
		>
		<RefreshControl onrefresh={data.refresh} loading={data.loading} />
	</div>
	<DataTable
		rows={chain?.rules ?? []}
		{columns}
		rowKey={(r) => String(r.number)}
		busy={busy.keys}
		loading={data.loading}
		error={data.error}
		onretry={data.refresh}
		empty="No rules in this chain"
		class="flex-1"
	>
		{#snippet cell(rule, column)}
			{#if column.key === 'target'}
				<span
					class={rule.target === 'ACCEPT'
						? 'text-success'
						: rule.target === 'DROP' || rule.target === 'REJECT'
							? 'text-destructive'
							: ''}>{rule.target}</span
				>
			{:else}
				{column.value?.(rule)}
			{/if}
		{/snippet}
		{#snippet actions(rule)}
			<IconButton label="Delete rule" onclick={() => remove(rule)}><Trash2 /></IconButton>
		{/snippet}
	</DataTable>
	<p class="text-xs text-muted-foreground">Rules added here are lost on reboot unless you persist them.</p>
</div>

<Modal bind:open={rawOpen} title="iptables -t {table} -L -n --line-numbers" size="xl" class="h-[70vh]" flush>
	<LogViewer source={data.data?.raw ?? ''} downloadName="iptables-{table}.txt" class="flex-1" />
</Modal>

<Modal bind:open={addOpen} title="Add rule to {chain?.name}" size="lg">
	<form
		id="ipt-form"
		class="grid grid-cols-2 gap-3"
		onsubmit={(e) => {
			e.preventDefault();
			void add();
		}}
	>
		<Field label="Target"
			><SelectField bind:value={form.target} options={['ACCEPT', 'DROP', 'REJECT']} /></Field
		>
		<Field label="Protocol"
			><SelectField
				bind:value={form.protocol}
				options={[
					{ value: 'all', label: 'All' },
					{ value: 'tcp', label: 'TCP' },
					{ value: 'udp', label: 'UDP' },
					{ value: 'icmp', label: 'ICMP' }
				]}
			/></Field
		>
		<Field label="Source" hint="IP or CIDR. Empty means any."
			><Input bind:value={form.source} class="font-mono" spellcheck="false" /></Field
		>
		<Field label="Destination" hint="IP or CIDR. Empty means any."
			><Input bind:value={form.destination} class="font-mono" spellcheck="false" /></Field
		>
		<Field label="Destination port" hint="TCP and UDP only."
			><Input
				bind:value={form.port}
				class="font-mono"
				disabled={form.protocol !== 'tcp' && form.protocol !== 'udp'}
			/></Field
		>
		<Field label="Position">
			<div class="flex gap-2">
				<SelectField
					bind:value={form.position}
					options={[
						{ value: 'append', label: 'Append (end)' },
						{ value: 'insert', label: 'Insert (top)' },
						{ value: 'line', label: 'At line…' }
					]}
				/>
				{#if form.position === 'line'}<Input
						bind:value={form.line}
						class="w-20 font-mono"
						inputmode="numeric"
					/>{/if}
			</div>
		</Field>
	</form>
	{#snippet footer()}
		<Button variant="outline" onclick={() => (addOpen = false)}>Cancel</Button>
		<Button type="submit" form="ipt-form">Add rule</Button>
	{/snippet}
</Modal>
