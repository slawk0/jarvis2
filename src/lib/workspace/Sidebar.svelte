<script lang="ts">
	import { getVersion } from '@tauri-apps/api/app';
	import Check from '@lucide/svelte/icons/check';
	import ChevronsUpDown from '@lucide/svelte/icons/chevrons-up-down';
	import LogOut from '@lucide/svelte/icons/log-out';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import Star from '@lucide/svelte/icons/star';
	import StarOff from '@lucide/svelte/icons/star-off';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import IconButton from '$lib/components/IconButton.svelte';
	import SearchInput from '$lib/components/SearchInput.svelte';
	import Tip from '$lib/components/Tip.svelte';
	import { app } from '$lib/services/app.svelte';
	import { settings } from '$lib/services/settings.svelte';
	import { toast } from '$lib/services/toast.svelte';
	import { cn, copyText, matches } from '$lib/utils';
	import { drag } from './drag.svelte';
	import { CATEGORIES, TABS, TAB_COLORS, type TabColor, type TabDef } from './registry';
	import { workspace } from './workspace.svelte';

	let query = $state('');
	let version = $state('');
	void getVersion().then((v) => (version = v));

	const collapsed = $derived(settings.value.sidebarCollapsed);
	const favourites = $derived(
		settings.value.favourites
			.map((id) => TABS.find((t) => t.id === id))
			.filter((t): t is TabDef => t !== undefined && matches(query, t.label))
	);
	const groups = $derived(
		CATEGORIES.map((category) => ({
			category,
			tabs: TABS.filter((t) => t.category === category && matches(query, t.label, category))
		})).filter((g) => g.tabs.length > 0)
	);
	const activeTab = $derived(workspace.focused?.tab);
	const others = $derived(app.profiles.filter((p) => p.profile.id !== app.session?.profileId));

	const STATUS = {
		online: { label: 'Online', dot: 'bg-success' },
		offline: { label: 'Offline', dot: 'bg-destructive' },
		reconnecting: { label: 'Reconnecting…', dot: 'bg-warning animate-pulse' },
		switching: { label: 'Switching…', dot: 'bg-info animate-pulse' }
	};
	const status = $derived(STATUS[app.status]);

	const TAG_BG: Record<TabColor, string> = {
		red: 'bg-tag-red',
		orange: 'bg-tag-orange',
		yellow: 'bg-tag-yellow',
		green: 'bg-tag-green',
		teal: 'bg-tag-teal',
		blue: 'bg-tag-blue',
		purple: 'bg-tag-purple',
		pink: 'bg-tag-pink'
	};

	const isFavourite = (id: string) => settings.value.favourites.includes(id);
	const colorOf = (id: string) => settings.value.tabColors[id] as TabColor | undefined;

	function toggleFavourite(id: string): void {
		settings.update((s) => {
			s.favourites = isFavourite(id) ? s.favourites.filter((f) => f !== id) : [...s.favourites, id];
		});
	}

	function setColor(id: string, color: TabColor | null): void {
		settings.update((s) => {
			if (color) s.tabColors[id] = color;
			else delete s.tabColors[id];
		});
	}

	function toggleCollapsed(): void {
		settings.update((s) => (s.sidebarCollapsed = !s.sidebarCollapsed));
	}

	async function copyHost(): Promise<void> {
		if (!app.session) return;
		await copyText(app.session.host);
		toast.success('Host copied');
	}
</script>

{#snippet tabButton(t: TabDef)}
	{@const color = colorOf(t.id)}
	<ContextMenu.Root>
		<ContextMenu.Trigger>
			{#snippet child({ props })}
				<Tip text={collapsed ? t.label : null} side="right" class="block">
					<button
						{...props}
						type="button"
						class={cn(
							'group relative flex h-7.5 w-full items-center gap-2.5 rounded-md text-sm transition-colors',
							collapsed ? 'justify-center px-0' : 'px-2.5',
							activeTab === t.id
								? 'bg-primary/12 text-foreground font-medium'
								: 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
						)}
						aria-current={activeTab === t.id ? 'page' : undefined}
						onpointerdown={(e) => drag.begin(e, { kind: 'tab', tabId: t.id }, t.label)}
						onclick={() => workspace.openTab(t.id)}
					>
						{#if color}
							<span class={cn('absolute top-1.5 bottom-1.5 left-0 w-[3px] rounded-full', TAG_BG[color])}></span>
						{/if}
						<t.icon class={cn('size-4 shrink-0', activeTab === t.id && 'text-primary')} />
						{#if !collapsed}<span class="truncate">{t.label}</span>{/if}
					</button>
				</Tip>
			{/snippet}
		</ContextMenu.Trigger>
		<ContextMenu.Content class="w-52">
			<ContextMenu.Item onclick={() => workspace.openInNewPane(t.id)} disabled={!workspace.canSplit}>
				Open in new pane
			</ContextMenu.Item>
			<ContextMenu.Item onclick={() => toggleFavourite(t.id)}>
				{#if isFavourite(t.id)}<StarOff /> Remove from favourites{:else}<Star /> Add to favourites{/if}
			</ContextMenu.Item>
			<ContextMenu.Separator />
			<div class="flex items-center gap-1.5 px-2 py-1.5">
				{#each TAB_COLORS as c (c)}
					<button
						type="button"
						class={cn(
							'flex size-4 items-center justify-center rounded-full transition-transform hover:scale-110',
							TAG_BG[c]
						)}
						aria-label="Colour tag {c}"
						onclick={() => setColor(t.id, c)}
					>
						{#if color === c}<Check class="size-3 text-white" />{/if}
					</button>
				{/each}
			</div>
			<ContextMenu.Item disabled={!color} onclick={() => setColor(t.id, null)}>Clear colour</ContextMenu.Item>
		</ContextMenu.Content>
	</ContextMenu.Root>
{/snippet}

<aside
	class={cn(
		'bg-sidebar flex h-full shrink-0 flex-col border-r transition-[width] duration-150',
		collapsed ? 'w-14' : 'w-60'
	)}
>
	<Tip text={collapsed ? 'Expand sidebar (Ctrl+Alt+B)' : 'Collapse sidebar (Ctrl+Alt+B)'} side="right" class="block">
		<button
			type="button"
			class={cn('flex h-12 w-full shrink-0 items-center gap-2.5', collapsed ? 'justify-center' : 'px-3.5')}
			onclick={toggleCollapsed}
		>
			<img src="/logo.svg" alt="" class="size-7" />
			{#if !collapsed}
				<span class="text-[15px] font-semibold tracking-tight">Jarvis</span>
				<span class="text-muted-foreground text-xs">Server Manager</span>
			{/if}
		</button>
	</Tip>

	{#if !collapsed}
		<div class="px-2.5 pb-2">
			<SearchInput bind:value={query} placeholder="Search tabs…" class="w-full" />
		</div>
	{/if}

	<nav class={cn('min-h-0 flex-1 overflow-y-auto pb-2', collapsed ? 'px-2' : 'px-2.5')} aria-label="Tabs">
		{#if favourites.length > 0}
			{#if !collapsed}
				<h3 class="text-muted-foreground px-2.5 pt-1 pb-1 text-[11px] font-medium tracking-wide uppercase">Favourites</h3>
			{/if}
			<div class="flex flex-col gap-px">
				{#each favourites as t (t.id)}{@render tabButton(t)}{/each}
			</div>
			{#if collapsed}<div class="bg-border mx-1 my-2 h-px"></div>{/if}
		{/if}
		{#each groups as group (group.category)}
			{#if !collapsed}
				<h3 class="text-muted-foreground px-2.5 pt-3 pb-1 text-[11px] font-medium tracking-wide uppercase">
					{group.category}
				</h3>
			{:else}
				<div class="h-1.5"></div>
			{/if}
			<div class="flex flex-col gap-px">
				{#each group.tabs as t (t.id)}{@render tabButton(t)}{/each}
			</div>
		{/each}
		{#if groups.length === 0 && favourites.length === 0}
			<p class="text-muted-foreground px-2.5 py-4 text-xs">No tabs match “{query}”.</p>
		{/if}
	</nav>

	<footer class={cn('shrink-0 border-t', collapsed ? 'flex flex-col items-center gap-1 py-2' : 'p-2.5')}>
		{#if collapsed}
			<Tip text="{app.profile?.label ?? ''} · {status.label}" side="right">
				<span class={cn('my-1 size-2.5 rounded-full', status.dot)}></span>
			</Tip>
			{#if app.status === 'offline' || app.status === 'reconnecting'}
				<IconButton label="Reconnect now" side="right" onclick={() => app.reconnectNow()}><RotateCw /></IconButton>
			{/if}
			<IconButton label="Disconnect" side="right" onclick={() => app.disconnect()}><LogOut /></IconButton>
		{:else}
			<div class="flex items-center gap-2">
				<span class={cn('size-2 shrink-0 rounded-full', status.dot)}></span>
				{#if others.length > 0}
					<DropdownMenu.Root>
						<DropdownMenu.Trigger
							class="hover:bg-muted flex min-w-0 flex-1 items-center gap-1 rounded px-1 py-0.5 text-left text-sm font-medium"
						>
							<span class="truncate">{app.profile?.label}</span>
							<ChevronsUpDown class="text-muted-foreground size-3 shrink-0" />
						</DropdownMenu.Trigger>
						<DropdownMenu.Content align="start" class="w-56">
							<DropdownMenu.Label class="text-muted-foreground text-xs">Switch server</DropdownMenu.Label>
							{#each others as p (p.profile.id)}
								<DropdownMenu.Item onclick={() => app.connect(p.profile.id)}>
									<div class="min-w-0">
										<p class="truncate">{p.profile.label}</p>
										<p class="text-muted-foreground truncate text-xs">
											{p.profile.username}@{p.profile.host}
										</p>
									</div>
								</DropdownMenu.Item>
							{/each}
						</DropdownMenu.Content>
					</DropdownMenu.Root>
				{:else}
					<span class="min-w-0 flex-1 truncate px-1 text-sm font-medium">{app.profile?.label}</span>
				{/if}
				<IconButton label="Disconnect" onclick={() => app.disconnect()}><LogOut /></IconButton>
			</div>
			<div class="mt-1 flex items-center gap-2 pl-4 text-xs">
				<span class="text-muted-foreground">{status.label}</span>
				<Tip text="Copy host">
					<button type="button" class="text-muted-foreground hover:text-foreground min-w-0 truncate font-mono" onclick={copyHost}>
						{app.session?.host}
					</button>
				</Tip>
				{#if app.status === 'offline' || app.status === 'reconnecting'}
					<button type="button" class="text-primary ml-auto shrink-0 hover:underline" onclick={() => app.reconnectNow()}>
						Reconnect
					</button>
				{/if}
			</div>
			<p class="text-muted-foreground/70 mt-1.5 pl-4 text-[11px]">v{version}</p>
		{/if}
	</footer>
</aside>
