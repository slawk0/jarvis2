<!-- SQL editor: run a script (Ctrl+Enter), one result per statement, with history. -->
<script lang="ts">
	import History from '@lucide/svelte/icons/history';
	import Play from '@lucide/svelte/icons/play';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import CodeEditor from '$lib/editor/CodeEditor.svelte';
	import { api, toIpcError, type IpcError, type StatementResult } from '$lib/ipc';
	import { cn } from '$lib/utils';
	import ResultGrid from './ResultGrid.svelte';

	interface Props {
		connection: string;
		database: string;
		/** Called after a script ran (the schema may have changed). */
		onran?: () => void;
	}

	let { connection, database, onran }: Props = $props();

	const HISTORY_KEY = 'jarvis.sql-history';
	const HISTORY_MAX = 50;

	function loadHistory(): string[] {
		try {
			const parsed: unknown = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? '[]');
			return Array.isArray(parsed) ? parsed.filter((x) => typeof x === 'string') : [];
		} catch {
			return [];
		}
	}

	let script = $state('');
	let results = $state<StatementResult[] | null>(null);
	let error = $state<IpcError | null>(null);
	let running = $state(false);
	let history = $state(loadHistory());

	async function run(): Promise<void> {
		const text = script.trim();
		if (!text || running) return;
		running = true;
		error = null;
		try {
			results = await api.dbQuery(connection, database, text);
			history = [text, ...history.filter((h) => h !== text)].slice(0, HISTORY_MAX);
			localStorage.setItem(HISTORY_KEY, JSON.stringify(history));
			onran?.();
		} catch (raw) {
			error = toIpcError(raw);
			results = null;
		} finally {
			running = false;
		}
	}

	const oneLine = (text: string) => text.replace(/\s+/g, ' ').slice(0, 90);
</script>

<div class="flex min-h-0 flex-1 flex-col gap-2">
	<div class="flex items-center gap-2">
		<Button size="sm" disabled={running || !script.trim()} onclick={run}
			><Play /> {running ? 'Running…' : 'Run'}</Button
		>
		<span class="text-xs text-muted-foreground"
			>Ctrl+Enter · runs every statement in order and stops at the first error</span
		>
		<span class="flex-1"></span>
		<span class="font-mono text-xs text-muted-foreground">{database || 'no database selected'}</span>
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<Button {...props} variant="outline" size="sm" disabled={history.length === 0}
						><History /> History</Button
					>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="end" class="max-h-96 w-[32rem] overflow-y-auto">
				{#each history as entry, i (i)}
					<DropdownMenu.Item class="font-mono text-xs" onclick={() => (script = entry)}
						><span class="truncate">{oneLine(entry)}</span></DropdownMenu.Item
					>
				{/each}
				<DropdownMenu.Separator />
				<DropdownMenu.Item
					onclick={() => {
						history = [];
						localStorage.removeItem(HISTORY_KEY);
					}}>Clear history</DropdownMenu.Item
				>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</div>
	<CodeEditor
		bind:value={script}
		language="sql"
		onrun={run}
		class="h-48 shrink-0 overflow-hidden rounded-lg border"
	/>

	<div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
		{#if error}
			<p class="selectable text-sm text-destructive" role="alert">{error.message}</p>
		{:else if !results}
			<p class="text-sm text-muted-foreground">Results appear here.</p>
		{:else}
			{#each results as item, i (i)}
				<section class="flex flex-col gap-1">
					<div class="flex items-baseline gap-2">
						<span
							class={cn(
								'size-2 shrink-0 self-center rounded-full',
								item.error ? 'bg-destructive' : 'bg-success'
							)}
						></span>
						<code class="selectable min-w-0 flex-1 truncate text-xs" title={item.statement}
							>{oneLine(item.statement)}</code
						>
						<span class="text-xs text-muted-foreground tabular">{item.elapsedMs} ms</span>
					</div>
					{#if item.error}
						<p
							class="selectable rounded-md bg-destructive/10 px-3 py-2 font-mono text-xs whitespace-pre-wrap text-destructive"
							role="alert"
						>
							{item.error}
						</p>
					{:else if item.result}
						<ResultGrid result={item.result} name="query-{i + 1}" class="max-h-96" />
					{:else}
						<p class="text-xs text-muted-foreground">
							{item.affected ?? 0} row{item.affected === 1 ? '' : 's'} affected
						</p>
					{/if}
				</section>
			{/each}
		{/if}
	</div>
</div>
