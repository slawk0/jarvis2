<!-- A read-only grid for a query result, with CSV / JSON export. -->
<script lang="ts" module>
	import type { ResultSet } from '$lib/ipc';

	function csvField(value: string | null): string {
		if (value === null) return '';
		return /[",\r\n]/.test(value) ? `"${value.replaceAll('"', '""')}"` : value;
	}

	export function toCsv(result: ResultSet): string {
		const lines = [result.columns.map(csvField).join(',')];
		for (const row of result.rows) lines.push(row.map(csvField).join(','));
		return lines.join('\r\n') + '\r\n';
	}

	export function toJson(result: ResultSet): string {
		// Duplicate column names (joins) must not overwrite each other.
		const seen: Record<string, number> = {};
		const names = result.columns.map((name) => {
			seen[name] = (seen[name] ?? 0) + 1;
			return seen[name] === 1 ? name : `${name}_${seen[name]}`;
		});
		return JSON.stringify(
			result.rows.map((row) => Object.fromEntries(row.map((value, i) => [names[i], value]))),
			null,
			2
		);
	}
</script>

<script lang="ts">
	import { save as saveDialog } from '@tauri-apps/plugin-dialog';
	import { Button } from '$lib/components/ui/button';
	import { api } from '$lib/ipc';
	import { toast } from '$lib/services/toast.svelte';
	import { cn } from '$lib/utils';

	interface Props {
		result: ResultSet;
		name?: string;
		class?: string;
	}

	let { result, name = 'result', class: className }: Props = $props();

	async function exportAs(format: 'csv' | 'json'): Promise<void> {
		const path = await saveDialog({
			defaultPath: `${name}.${format}`,
			filters: [{ name: format.toUpperCase(), extensions: [format] }]
		});
		if (!path) return;
		try {
			await api.saveTextFile(path, format === 'csv' ? toCsv(result) : toJson(result));
			toast.success(`${result.rows.length} rows exported`);
		} catch (e) {
			toast.error(e, 'Export failed');
		}
	}
</script>

<div class={cn('flex min-h-0 flex-col gap-1', className)}>
	<div class="min-h-0 flex-1 overflow-auto rounded-lg border bg-card">
		<table class="w-max min-w-full border-separate border-spacing-0 text-sm">
			<thead class="sticky top-0 z-10">
				<tr>
					{#each result.columns as column, i (i)}
						<th
							class="h-8 border-b bg-card px-3 text-left text-xs font-medium whitespace-nowrap text-muted-foreground"
							title={result.columnTypes[i]}>{column}</th
						>
					{/each}
				</tr>
			</thead>
			<tbody>
				{#each result.rows as row, r (r)}
					<tr class="h-7 hover:bg-muted/40">
						{#each row as value, i (i)}
							<td
								class="selectable max-w-96 truncate border-b border-border/50 px-3 font-mono text-xs"
								title={value !== null && value.length > 40 ? value.slice(0, 2000) : undefined}
							>
								{#if value === null}<span class="text-muted-foreground/70 italic">NULL</span
									>{:else}{value}{/if}
							</td>
						{/each}
					</tr>
				{:else}
					<tr
						><td
							colspan={Math.max(1, result.columns.length)}
							class="px-3 py-4 text-center text-xs text-muted-foreground">No rows</td
						></tr
					>
				{/each}
			</tbody>
		</table>
	</div>
	<div class="flex items-center gap-2 text-xs text-muted-foreground">
		<span class="tabular"
			>{result.rows.length} row{result.rows.length === 1 ? '' : 's'}{result.truncated
				? ' (more exist; only the first ones are shown)'
				: ''}</span
		>
		<span class="flex-1"></span>
		<Button variant="ghost" size="xs" onclick={() => exportAs('csv')}>Export CSV</Button>
		<Button variant="ghost" size="xs" onclick={() => exportAs('json')}>Export JSON</Button>
	</div>
</div>
