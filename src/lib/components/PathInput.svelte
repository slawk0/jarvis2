<!-- Remote path input with autocomplete (arrows / Tab / Enter / Esc). -->
<script lang="ts">
	import Folder from '@lucide/svelte/icons/folder';
	import FileIcon from '@lucide/svelte/icons/file';
	import { Input } from '$lib/components/ui/input';
	import { apiQuiet } from '$lib/ipc';
	import { cn, debounce } from '$lib/utils';

	interface Props {
		value: string;
		dirsOnly?: boolean;
		placeholder?: string;
		id?: string;
		autofocus?: boolean;
		class?: string;
		/** Enter was pressed with no suggestion highlighted. */
		onsubmit?: (value: string) => void;
	}

	let {
		value = $bindable(''),
		dirsOnly = false,
		placeholder = '/path/on/server',
		id,
		autofocus = false,
		class: className,
		onsubmit
	}: Props = $props();

	let suggestions = $state<string[]>([]);
	let open = $state(false);
	let active = $state(-1);
	let seq = 0;

	const fetchSuggestions = debounce(async (input: string) => {
		const mine = ++seq;
		try {
			const found = await apiQuiet.filesComplete(input, dirsOnly);
			if (mine !== seq) return;
			suggestions = found.filter((s) => s !== input);
			active = -1;
			open = suggestions.length > 0;
		} catch {
			suggestions = [];
			open = false;
		}
	}, 150);

	function pick(suggestion: string): void {
		value = suggestion;
		open = false;
		if (suggestion.endsWith('/')) fetchSuggestions(suggestion);
	}

	function onkeydown(e: KeyboardEvent): void {
		if (open && suggestions.length > 0) {
			if (e.key === 'ArrowDown') {
				e.preventDefault();
				active = (active + 1) % suggestions.length;
				return;
			}
			if (e.key === 'ArrowUp') {
				e.preventDefault();
				active = (active - 1 + suggestions.length) % suggestions.length;
				return;
			}
			if (e.key === 'Tab') {
				e.preventDefault();
				pick(suggestions[Math.max(active, 0)]);
				return;
			}
			if (e.key === 'Enter' && active >= 0) {
				e.preventDefault();
				pick(suggestions[active]);
				return;
			}
			if (e.key === 'Escape') {
				e.preventDefault();
				e.stopPropagation();
				open = false;
				return;
			}
		}
		if (e.key === 'Enter') {
			e.preventDefault();
			open = false;
			onsubmit?.(value.trim());
		}
	}
</script>

<div class={cn('relative', className)}>
	<Input
		{id}
		bind:value
		{placeholder}
		{autofocus}
		class="font-mono text-xs"
		spellcheck="false"
		autocomplete="off"
		role="combobox"
		aria-expanded={open}
		aria-autocomplete="list"
		oninput={() => fetchSuggestions(value)}
		onfocus={() => fetchSuggestions(value)}
		onblur={() => setTimeout(() => (open = false), 120)}
		{onkeydown}
	/>
	{#if open}
		<ul
			class="absolute top-full right-0 left-0 z-50 mt-1 max-h-60 overflow-y-auto rounded-lg border bg-popover p-1 shadow-lg"
			role="listbox"
		>
			{#each suggestions as suggestion, i (suggestion)}
				<li role="option" aria-selected={i === active}>
					<button
						type="button"
						class={cn(
							'flex w-full items-center gap-2 rounded-md px-2 py-1 text-left font-mono text-xs',
							i === active ? 'bg-accent' : 'hover:bg-muted'
						)}
						onmousedown={(e) => {
							e.preventDefault();
							pick(suggestion);
						}}
					>
						{#if suggestion.endsWith('/')}
							<Folder class="size-3.5 shrink-0 text-primary" />
						{:else}
							<FileIcon class="size-3.5 shrink-0 text-muted-foreground" />
						{/if}
						<span class="truncate">{suggestion}</span>
					</button>
				</li>
			{/each}
		</ul>
	{/if}
</div>
