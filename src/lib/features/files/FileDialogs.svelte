<!-- Permissions, owner/group and properties dialogs of the file browser. -->
<script lang="ts" module>
	import type { FileEntry } from '$lib/ipc';

	export interface FileDialog {
		kind: 'permissions' | 'owner' | 'properties';
		entries: FileEntry[];
	}
</script>

<script lang="ts">
	import Field from '$lib/components/Field.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { formatBytes, formatDateTime, formatMode, formatOctal } from '$lib/format';
	import { api, type FileProperties } from '$lib/ipc';
	import { sudo } from '$lib/services/sudo.svelte';
	import { toast } from '$lib/services/toast.svelte';

	interface Props {
		dialog: FileDialog | null;
		onchanged: () => void;
	}

	let { dialog = $bindable(null), onchanged }: Props = $props();

	let mode = $state(0o644);
	let octal = $state('644');
	let recursive = $state(false);
	let owner = $state('');
	let group = $state('');
	let busy = $state(false);
	let properties = $state<FileProperties | null>(null);
	let totalSize = $state<number | null>(null);
	let sizing = $state(false);

	const first = $derived(dialog?.entries[0]);
	const many = $derived((dialog?.entries.length ?? 0) > 1);
	const hasDirs = $derived(dialog?.entries.some((e) => e.kind === 'dir') ?? false);
	const title = $derived(many ? `${dialog?.entries.length} items` : (first?.name ?? ''));

	$effect(() => {
		const current = dialog;
		if (!current) return;
		const entry = current.entries[0];
		mode = entry.mode & 0o777;
		octal = formatOctal(entry.mode);
		recursive = false;
		owner = current.entries.length === 1 ? entry.owner : '';
		group = current.entries.length === 1 ? entry.group : '';
		properties = null;
		totalSize = null;
		if (current.kind === 'properties') {
			api.filesProperties(entry.path).then(
				(p) => (properties = p),
				(e) => toast.error(e)
			);
		}
	});

	const BITS = [
		{ who: 'Owner', shift: 6 },
		{ who: 'Group', shift: 3 },
		{ who: 'Others', shift: 0 }
	];
	const RWX = [
		{ label: 'Read', bit: 4 },
		{ label: 'Write', bit: 2 },
		{ label: 'Execute', bit: 1 }
	];

	function toggle(shift: number, bit: number): void {
		mode ^= bit << shift;
		octal = mode.toString(8).padStart(3, '0');
	}

	function onOctal(value: string): void {
		octal = value;
		if (/^[0-7]{3,4}$/.test(value)) mode = parseInt(value, 8);
	}

	const octalValid = $derived(/^[0-7]{3,4}$/.test(octal));
	const close = () => (dialog = null);

	async function apply(action: () => Promise<boolean>, done: string): Promise<void> {
		busy = true;
		try {
			const elevated = await action();
			toast.success(elevated ? `${done} (as root)` : done);
			close();
			onchanged();
		} catch (error) {
			toast.error(error);
		} finally {
			busy = false;
		}
	}

	const paths = () => dialog?.entries.map((e) => e.path) ?? [];

	async function calculate(): Promise<void> {
		if (!first) return;
		sizing = true;
		try {
			totalSize = (await api.filesSizes([first.path]))[0]?.bytes ?? null;
		} catch (error) {
			toast.error(error);
		} finally {
			sizing = false;
		}
	}
</script>

{#if dialog?.kind === 'permissions'}
	<Modal open title="Permissions · {title}" size="sm" onclose={close}>
		<div class="flex flex-col gap-3">
			<table class="w-full text-sm">
				<thead class="text-xs text-muted-foreground">
					<tr
						><th></th>{#each RWX as p (p.bit)}<th class="pb-1 font-medium">{p.label}</th>{/each}</tr
					>
				</thead>
				<tbody>
					{#each BITS as row (row.shift)}
						<tr>
							<td class="py-1.5">{row.who}</td>
							{#each RWX as p (p.bit)}
								<td class="text-center">
									<Checkbox
										checked={(mode & (p.bit << row.shift)) !== 0}
										onCheckedChange={() => toggle(row.shift, p.bit)}
										aria-label="{row.who} {p.label}"
									/>
								</td>
							{/each}
						</tr>
					{/each}
				</tbody>
			</table>
			<Field label="Octal" error={octalValid ? null : 'Three or four digits from 0 to 7.'}>
				<div class="flex items-center gap-3">
					<Input
						value={octal}
						oninput={(e) => onOctal(e.currentTarget.value)}
						class="w-24 font-mono"
						maxlength={4}
					/>
					<span class="font-mono text-xs text-muted-foreground">{formatMode(mode)}</span>
				</div>
			</Field>
			{#if hasDirs}
				<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={recursive} /> Apply to everything inside folders</label
				>
			{/if}
		</div>
		{#snippet footer()}
			<Button variant="outline" onclick={close}>Cancel</Button>
			<Button
				disabled={busy || !octalValid}
				onclick={() =>
					apply(() => api.filesChmod(paths(), parseInt(octal, 8), recursive), 'Permissions changed')}
				>Apply</Button
			>
		{/snippet}
	</Modal>
{:else if dialog?.kind === 'owner'}
	<Modal
		open
		title="Owner and group · {title}"
		description="Changing ownership requires root."
		size="sm"
		onclose={close}
	>
		<div class="flex flex-col gap-3">
			<Field label="Owner" hint="Leave empty to keep the current owner."
				><Input bind:value={owner} spellcheck="false" autocomplete="off" /></Field
			>
			<Field label="Group" hint="Leave empty to keep the current group."
				><Input bind:value={group} spellcheck="false" autocomplete="off" /></Field
			>
			{#if hasDirs}
				<label class="flex items-center gap-2 text-sm"
					><Checkbox bind:checked={recursive} /> Apply to everything inside folders</label
				>
			{/if}
		</div>
		{#snippet footer()}
			<Button variant="outline" onclick={close}>Cancel</Button>
			<Button
				disabled={busy || (!owner.trim() && !group.trim())}
				onclick={() =>
					apply(
						() =>
							sudo.describe('Change file owner', () =>
								api.filesChown(paths(), owner.trim(), group.trim(), recursive)
							),
						'Owner changed'
					)}
			>
				Apply
			</Button>
		{/snippet}
	</Modal>
{:else if dialog?.kind === 'properties' && first}
	{@const entry = properties?.entry ?? first}
	<Modal open title="Properties · {first.name}" size="md" onclose={close}>
		<dl class="selectable grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
			<dt class="text-muted-foreground">Type</dt>
			<dd>
				{entry.kind === 'dir'
					? 'Folder'
					: entry.kind === 'symlink'
						? 'Symbolic link'
						: entry.kind === 'file'
							? 'File'
							: 'Special file'}{#if properties?.mime}
					<span class="text-muted-foreground">· {properties.mime}</span>{/if}
			</dd>
			<dt class="text-muted-foreground">Path</dt>
			<dd class="font-mono text-xs break-all">{first.path}</dd>
			{#if properties?.linkTarget}
				<dt class="text-muted-foreground">Link target</dt>
				<dd class="font-mono text-xs break-all">{properties.linkTarget}</dd>
			{/if}
			<dt class="text-muted-foreground">Size</dt>
			<dd class="flex items-center gap-2">
				{#if entry.kind === 'dir'}
					{totalSize !== null ? formatBytes(totalSize) : '—'}
					<Button variant="outline" size="xs" disabled={sizing} onclick={calculate}
						>{sizing ? 'Calculating…' : 'Calculate'}</Button
					>
				{:else}
					{formatBytes(entry.size)}
					<span class="text-xs text-muted-foreground tabular">({entry.size.toLocaleString()} bytes)</span>
				{/if}
			</dd>
			<dt class="text-muted-foreground">Permissions</dt>
			<dd class="font-mono text-xs">{formatOctal(entry.mode)} {formatMode(entry.mode)}</dd>
			<dt class="text-muted-foreground">Owner</dt>
			<dd>{entry.owner}:{entry.group}</dd>
			<dt class="text-muted-foreground">Modified</dt>
			<dd>{formatDateTime(entry.modified, true)}</dd>
		</dl>
		{#snippet footer()}
			<Button variant="outline" onclick={close}>Close</Button>
		{/snippet}
	</Modal>
{/if}
