<!-- First-connection fingerprint prompt and the blocking "key changed" warning. -->
<script lang="ts">
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import ShieldQuestion from '@lucide/svelte/icons/shield-question';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import Modal from '$lib/components/Modal.svelte';
	import { app } from '$lib/services/app.svelte';

	const prompt = $derived(app.hostKeyPrompt);
	const changed = $derived((prompt?.issue.knownFingerprints.length ?? 0) > 0);
	let understood = $state(false);

	$effect(() => {
		void prompt;
		understood = false;
	});
</script>

{#if prompt}
	{@const issue = prompt.issue}
	<Modal
		open
		title={changed ? 'Warning: the host key has changed' : 'Trust this server?'}
		size="md"
		onclose={() => app.rejectHostKey()}
	>
		<div class="flex flex-col gap-3 text-sm">
			<div class="flex items-start gap-3">
				{#if changed}
					<ShieldAlert class="text-destructive mt-0.5 size-6 shrink-0" />
					<p>
						<strong>{issue.host}:{issue.port}</strong> presented a different key than the one you trusted before.
						This can mean the server was reinstalled — or that someone is intercepting the connection.
						Only continue if you know why the key changed.
					</p>
				{:else}
					<ShieldQuestion class="text-primary mt-0.5 size-6 shrink-0" />
					<p>
						This is the first connection to <strong>{issue.host}:{issue.port}</strong>. Check that the fingerprint
						matches the server before trusting it.
					</p>
				{/if}
			</div>

			<dl class="bg-sunken selectable flex flex-col gap-2 rounded-md border p-3 text-xs">
				{#if changed}
					<div>
						<dt class="text-muted-foreground">Previously trusted</dt>
						{#each issue.knownFingerprints as fp (fp)}
							<dd class="font-mono break-all">{fp}</dd>
						{/each}
					</div>
				{/if}
				<div>
					<dt class="text-muted-foreground">{changed ? 'New fingerprint' : 'Fingerprint'} ({issue.keyType})</dt>
					<dd class="font-mono break-all">{issue.fingerprint}</dd>
				</div>
			</dl>

			{#if changed}
				<label class="flex items-start gap-2">
					<Checkbox bind:checked={understood} class="mt-0.5" />
					<span>I understand the risk and want to replace the trusted key.</span>
				</label>
			{/if}
		</div>
		{#snippet footer()}
			<Button variant="outline" onclick={() => app.rejectHostKey()}>Cancel</Button>
			{#if changed}
				<Button variant="destructive" disabled={!understood} onclick={() => app.trustHostKey()}>
					Replace key and connect
				</Button>
			{:else}
				<Button onclick={() => app.trustHostKey()}>Trust and connect</Button>
			{/if}
		{/snippet}
	</Modal>
{/if}
