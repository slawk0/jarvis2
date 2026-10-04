<!-- The one sudo prompt. Mounted once; opened by the sudo service. -->
<script lang="ts">
	import KeyRound from '@lucide/svelte/icons/key-round';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { app } from '$lib/services/app.svelte';
	import { sudo } from '$lib/services/sudo.svelte';
	import Modal from './Modal.svelte';

	let password = $state('');

	$effect(() => {
		if (!sudo.open) password = '';
	});

	async function submit(): Promise<void> {
		await sudo.submit(password);
		if (sudo.open) password = '';
	}
</script>

{#if sudo.open}
	<Modal
		open
		title="Administrator password required"
		description={sudo.expired
			? 'Your sudo session expired. Enter the password again to continue.'
			: `Enter the sudo password for ${app.session?.user ?? 'this user'}. The action continues automatically.`}
		size="sm"
		onclose={() => sudo.cancel()}
	>
		<form
			id="sudo-form"
			class="flex flex-col gap-3"
			onsubmit={(e) => {
				e.preventDefault();
				void submit();
			}}
		>
			{#if sudo.action}
				<div class="flex items-center gap-2 rounded-md border bg-sunken px-2.5 py-2 text-xs">
					<KeyRound class="size-3.5 shrink-0 text-primary" />
					<span class="text-muted-foreground">Needed for:</span>
					<span class="truncate font-medium">{sudo.action}</span>
				</div>
			{/if}
			<Input
				type="password"
				bind:value={password}
				placeholder="sudo password"
				autocomplete="off"
				autofocus
				disabled={sudo.busy || sudo.lockedFor > 0}
				aria-invalid={sudo.error ? 'true' : undefined}
			/>
			{#if sudo.lockedFor > 0}
				<p class="text-xs text-destructive" role="alert">
					Too many incorrect attempts. Locked for {sudo.lockedFor} s.
				</p>
			{:else if sudo.error}
				<p class="text-xs text-destructive" role="alert">{sudo.error}</p>
			{/if}
		</form>
		{#snippet footer()}
			<Button variant="outline" onclick={() => sudo.cancel()}>Cancel</Button>
			<Button type="submit" form="sudo-form" disabled={!password || sudo.busy || sudo.lockedFor > 0}>
				{sudo.busy ? 'Checking…' : 'Continue'}
			</Button>
		{/snippet}
	</Modal>
{/if}
