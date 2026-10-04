<!-- A plain value/options select on top of the shadcn Select primitive. -->
<script lang="ts" generics="T extends string">
	import * as Select from '$lib/components/ui/select';

	interface Option {
		value: T;
		label: string;
		disabled?: boolean;
	}

	interface Props {
		value: T;
		options: readonly (Option | T)[];
		placeholder?: string;
		disabled?: boolean;
		size?: 'sm' | 'default';
		class?: string;
		onchange?: (value: T) => void;
	}

	let {
		value = $bindable(),
		options,
		placeholder = 'Select…',
		disabled = false,
		size = 'default',
		class: className = 'w-full',
		onchange
	}: Props = $props();

	const items = $derived(options.map((o): Option => (typeof o === 'string' ? { value: o, label: o } : o)));
	const current = $derived(items.find((o) => o.value === value));
</script>

<Select.Root
	type="single"
	{disabled}
	bind:value={
		() => value as string,
		(v) => {
			value = v as T;
			onchange?.(v as T);
		}
	}
>
	<Select.Trigger class={className} {size}>
		{#if current}
			<span class="truncate">{current.label}</span>
		{:else}
			<span class="truncate text-muted-foreground">{placeholder}</span>
		{/if}
	</Select.Trigger>
	<Select.Content>
		{#each items as item (item.value)}
			<Select.Item value={item.value} label={item.label} disabled={item.disabled}>
				{item.label}
			</Select.Item>
		{/each}
	</Select.Content>
</Select.Root>
