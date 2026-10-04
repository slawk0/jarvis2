<!-- Renders the request held by the confirm/prompt service. Mounted once. -->
<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import { dialogs } from '$lib/services/confirm.svelte';
	import Modal from './Modal.svelte';

	const request = $derived(dialogs.current);
	let typed = $state('');
	let acknowledged = $state(false);
	let value = $state('');
	let touched = $state(false);

	// Reset the local inputs whenever a new request is shown.
	$effect(() => {
		const current = dialogs.current;
		typed = '';
		acknowledged = false;
		touched = false;
		value = current?.kind === 'prompt' ? (current.options.value ?? '') : '';
	});

	const confirmBlocked = $derived(
		request?.kind === 'confirm' &&
			((request.options.typeToConfirm !== undefined && typed !== request.options.typeToConfirm) ||
				(request.options.acknowledge !== undefined && !acknowledged))
	);
	const promptError = $derived(
		request?.kind === 'prompt' ? (request.options.validate?.(value) ?? null) : null
	);

	function submitPrompt(): void {
		touched = true;
		if (promptError === null) dialogs.settle(value);
	}
</script>

{#if request?.kind === 'confirm'}
	{@const o = request.options}
	<Modal open title={o.title} size="md" onclose={() => dialogs.settle(false)}>
		<div class="flex flex-col gap-3">
			{#if o.message}<p class="text-sm text-muted-foreground">{o.message}</p>{/if}
			{#if o.detail}
				<pre
					class="selectable max-h-60 overflow-auto rounded-md border bg-sunken p-2.5 text-xs whitespace-pre-wrap">{o.detail}</pre>
			{/if}
			{#if o.typeToConfirm !== undefined}
				<label class="flex flex-col gap-1.5 text-sm">
					<span class="text-muted-foreground">
						Type <code class="selectable font-semibold text-foreground">{o.typeToConfirm}</code> to confirm
					</span>
					<Input bind:value={typed} autocomplete="off" spellcheck="false" autofocus />
				</label>
			{/if}
			{#if o.acknowledge !== undefined}
				<label class="flex items-start gap-2 text-sm">
					<Checkbox bind:checked={acknowledged} class="mt-0.5" />
					<span>{o.acknowledge}</span>
				</label>
			{/if}
		</div>
		{#snippet footer()}
			<Button variant="outline" onclick={() => dialogs.settle(false)}>{o.cancelLabel ?? 'Cancel'}</Button>
			<Button
				variant={o.destructive ? 'destructive' : 'default'}
				disabled={confirmBlocked}
				onclick={() => dialogs.settle(true)}
			>
				{o.confirmLabel ?? 'Confirm'}
			</Button>
		{/snippet}
	</Modal>
{:else if request?.kind === 'prompt'}
	{@const o = request.options}
	<Modal open title={o.title} size="md" onclose={() => dialogs.settle(null)}>
		<form
			id="prompt-form"
			class="flex flex-col gap-2"
			onsubmit={(e) => {
				e.preventDefault();
				submitPrompt();
			}}
		>
			{#if o.message}<p class="text-sm text-muted-foreground">{o.message}</p>{/if}
			{#if o.label}<label for="prompt-input" class="text-xs font-medium text-muted-foreground"
					>{o.label}</label
				>{/if}
			{#if o.multiline}
				<Textarea
					id="prompt-input"
					bind:value
					placeholder={o.placeholder}
					rows={6}
					class="font-mono text-xs"
				/>
			{:else}
				<Input
					id="prompt-input"
					bind:value
					type={o.password ? 'password' : 'text'}
					placeholder={o.placeholder}
					autocomplete="off"
					spellcheck="false"
					autofocus
				/>
			{/if}
			{#if touched && promptError}<p class="text-xs text-destructive">{promptError}</p>{/if}
		</form>
		{#snippet footer()}
			<Button variant="outline" onclick={() => dialogs.settle(null)}>Cancel</Button>
			<Button type="submit" form="prompt-form">{o.confirmLabel ?? 'OK'}</Button>
		{/snippet}
	</Modal>
{/if}
