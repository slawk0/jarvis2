<!-- Cron expression input: raw field, five preset dropdowns and a description. -->
<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { CRON_FIELDS, describeCron, isValidCron } from '$lib/cron';
	import SelectField from './SelectField.svelte';

	interface Props {
		value: string;
		id?: string;
	}

	let { value = $bindable('0 3 * * *'), id }: Props = $props();

	const fields = $derived(value.trim().split(/\s+/));
	const editable = $derived(fields.length === 5 && !value.trim().startsWith('@'));
	const description = $derived(describeCron(value));
	const valid = $derived(isValidCron(value));

	function setField(index: number, next: string): void {
		const parts = editable ? [...fields] : ['*', '*', '*', '*', '*'];
		parts[index] = next;
		value = parts.join(' ');
	}
</script>

<div class="flex flex-col gap-2">
	<Input
		{id}
		bind:value
		class="font-mono"
		spellcheck="false"
		autocomplete="off"
		aria-invalid={valid ? undefined : 'true'}
		placeholder="minute hour day month weekday"
	/>
	<div class="grid grid-cols-5 gap-1.5">
		{#each CRON_FIELDS as field, i (field.name)}
			{@const current = editable ? fields[i] : ''}
			<div class="flex flex-col gap-1">
				<span class="text-[11px] text-muted-foreground">{field.name}</span>
				<SelectField
					size="sm"
					value={current}
					placeholder={current || '—'}
					options={[
						...field.presets,
						...(current && !field.presets.some((p) => p.value === current)
							? [{ value: current, label: current }]
							: [])
					]}
					onchange={(v) => setField(i, v)}
				/>
			</div>
		{/each}
	</div>
	<p class={valid ? 'text-xs text-muted-foreground' : 'text-xs text-destructive'} role="status">
		{valid ? description : 'Not a valid cron expression (five fields, or a keyword such as @daily).'}
	</p>
</div>
