<!-- Add a row, or edit one: every column gets a value field and a "NULL" switch. -->
<script lang="ts">
	import Modal from '$lib/components/Modal.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { api, type ColumnInfo, type TableRef } from '$lib/ipc';
	import { toast } from '$lib/services/toast.svelte';

	interface Props {
		connection: string;
		table: TableRef;
		columns: ColumnInfo[];
		/** The row being edited, or `null` to add one. */
		row: (string | null)[] | null;
		rowKey: [string, string | null][];
		onclose: (changed: boolean) => void;
	}

	let { connection, table, columns, row, rowKey, onclose }: Props = $props();

	interface FieldState {
		value: string;
		isNull: boolean;
		/** New rows: leave the column out so the database default applies. */
		useDefault: boolean;
	}

	const adding = $derived(row === null);
	// svelte-ignore state_referenced_locally
	let fields = $state<FieldState[]>(
		columns.map((column, i) => {
			const current = row ? row[i] : null;
			const generated = column.extra.includes('auto_increment') || column.default !== null;
			return {
				value: current ?? '',
				isNull: row ? current === null : false,
				useDefault: !row && (generated || column.nullable)
			};
		})
	);
	let saving = $state(false);

	const long = (value: string) => value.length > 80 || value.includes('\n');

	async function save(): Promise<void> {
		saving = true;
		try {
			if (adding) {
				const values = columns
					.map((c, i): [string, string | null] | null =>
						fields[i].useDefault ? null : [c.name, fields[i].isNull ? null : fields[i].value]
					)
					.filter((v) => v !== null);
				await api.dbRowInsert(connection, $state.snapshot(table), values);
				toast.success('Row added');
			} else {
				const changes = columns
					.map((c, i): [string, string | null] | null => {
						const next = fields[i].isNull ? null : fields[i].value;
						return next === row![i] ? null : [c.name, next];
					})
					.filter((v) => v !== null);
				if (changes.length === 0) {
					onclose(false);
					return;
				}
				const affected = await api.dbRowUpdate(connection, $state.snapshot(table), rowKey, changes);
				if (affected === 0)
					toast.warning('No row was changed. It may have been modified or deleted meanwhile.');
				else toast.success(affected === 1 ? 'Row updated' : `${affected} identical rows updated`);
			}
			onclose(true);
		} catch (e) {
			toast.error(e, adding ? 'Could not add the row' : 'Could not update the row');
		} finally {
			saving = false;
		}
	}
</script>

<Modal
	open
	title={adding ? `Add a row to ${table.table}` : `Edit row in ${table.table}`}
	size="lg"
	onclose={() => onclose(false)}
>
	<div class="flex flex-col gap-3">
		{#each columns as column, i (column.name)}
			{@const field = fields[i]}
			{@const off = field.isNull || field.useDefault}
			<div class="grid grid-cols-[11rem_1fr] items-start gap-3">
				<div class="min-w-0 pt-1.5">
					<p class="truncate text-sm font-medium" title={column.name}>{column.name}</p>
					<p class="truncate font-mono text-[11px] text-muted-foreground" title={column.dataType}>
						{column.dataType}{column.key === 'PRI' ? ' · PK' : ''}
					</p>
				</div>
				<div class="flex min-w-0 flex-col gap-1">
					{#if long(field.value)}
						<Textarea
							bind:value={field.value}
							disabled={off}
							rows={4}
							class="font-mono text-xs"
							spellcheck="false"
						/>
					{:else}
						<Input
							bind:value={field.value}
							disabled={off}
							class="font-mono text-xs"
							spellcheck="false"
							placeholder={field.isNull ? 'NULL' : field.useDefault ? (column.default ?? 'default') : ''}
						/>
					{/if}
					<div class="flex items-center gap-4 text-xs text-muted-foreground">
						{#if column.nullable}
							<label class="flex items-center gap-1.5">
								<Checkbox
									checked={field.isNull}
									onCheckedChange={(v) => {
										field.isNull = v === true;
										if (field.isNull) field.useDefault = false;
									}}
								/> NULL
							</label>
						{/if}
						{#if adding}
							<label class="flex items-center gap-1.5">
								<Checkbox
									checked={field.useDefault}
									onCheckedChange={(v) => {
										field.useDefault = v === true;
										if (field.useDefault) field.isNull = false;
									}}
								/> Use default
							</label>
						{/if}
					</div>
				</div>
			</div>
		{/each}
	</div>
	{#snippet footer()}
		<Button variant="outline" onclick={() => onclose(false)}>Cancel</Button>
		<Button disabled={saving} onclick={save}
			>{saving ? 'Saving…' : adding ? 'Add row' : 'Save changes'}</Button
		>
	{/snippet}
</Modal>
