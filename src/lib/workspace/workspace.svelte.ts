/**
 * Workspace state: the pane tree, what each pane shows, focus, per-pane tab
 * history and the back-handler stack. Persisted per profile.
 */
import { getContext, setContext } from 'svelte';
import { loadDoc, saveDoc } from '$lib/services/profile-data';
import { confirm } from '$lib/services/confirm.svelte';
import { debounce, uid } from '$lib/utils';
import {
	MAX_PANES,
	buildPreset,
	countPanes,
	hasPane,
	movePane,
	pane as paneNode,
	paneIds,
	parseLayout,
	presetSize,
	removePane,
	setRatio,
	splitPane,
	swapPanes,
	visualOrder,
	type DropZone,
	type Edge,
	type LayoutNode,
	type Preset
} from './layout';
import { DEFAULT_TAB, TABS, isTabId, tabDef, type TabExports, type TabId } from './registry';

/** Cross-tab requests, e.g. "look up this IP" or "open a container shell". */
export interface TabRequests {
	terminal:
		| { kind: 'container'; container: string; shell: string }
		| { kind: 'edit'; path: string };
	netdiag: { ip: string };
	files: { path: string };
	logs: { unit: string };
	processes: Record<string, never>;
	disks: Record<string, never>;
	pangolin: Record<string, never>;
	firewall: Record<string, never>;
	backups: Record<string, never>;
	crowdsec: { view: string };
}

interface BackEntry {
	id: string;
	label: string;
	/** `null` for app-wide handlers (modals). */
	paneId: string | null;
	run: () => void;
}

class PaneState {
	readonly id: string;
	tab = $state<TabId>(DEFAULT_TAB);
	/** Tabs visited in this pane; they stay mounted so their state survives. */
	mounted = $state<TabId[]>([]);
	history: TabId[] = [];
	/** Reasons this pane must not be discarded silently (live terminal, unsaved editor). */
	live = $state<Record<string, string>>({});
	instances = new Map<TabId, TabExports>();

	constructor(id: string, tab: TabId) {
		this.id = id;
		this.tab = tab;
		this.mounted = [tab];
	}

	get liveReasons(): string[] {
		return Object.values(this.live);
	}
}

class Workspace {
	layout = $state<LayoutNode>(paneNode('p1'));
	panes = $state<Record<string, PaneState>>({});
	focusedId = $state('p1');
	backStack = $state<BackEntry[]>([]);
	shortcutsOpen = $state(false);
	settingsOpen = $state(false);
	/** Pending cross-tab requests, consumed by the receiving tab. */
	requests = $state<{ [K in keyof TabRequests]?: TabRequests[K] }>({});

	#save = debounce(() => void this.#persist(), 400);

	constructor() {
		this.reset();
	}

	// ------------------------------------------------------------ lifecycle

	reset(): void {
		const id = uid();
		this.layout = paneNode(id);
		this.panes = { [id]: new PaneState(id, DEFAULT_TAB) };
		this.focusedId = id;
		this.backStack = [];
		this.requests = {};
	}

	/** Restore the layout saved for the active profile (or start fresh). */
	async load(): Promise<void> {
		this.reset();
		try {
			const doc = await loadDoc('workspaceLayout', { version: 1, layout: null, focused: null, tabs: {} });
			const layout = parseLayout(doc.layout);
			if (!layout) return;
			const panes: Record<string, PaneState> = {};
			for (const id of paneIds(layout)) {
				const tab = doc.tabs?.[id];
				panes[id] = new PaneState(id, isTabId(tab) ? tab : DEFAULT_TAB);
			}
			this.layout = layout;
			this.panes = panes;
			this.focusedId = doc.focused && panes[doc.focused] ? doc.focused : paneIds(layout)[0];
		} catch {
			// A missing or unreadable layout is not worth interrupting the user for.
		}
	}

	async #persist(): Promise<void> {
		const tabs: Record<string, string> = {};
		for (const id of paneIds(this.layout)) tabs[id] = this.panes[id]?.tab ?? DEFAULT_TAB;
		try {
			await saveDoc('workspaceLayout', {
				version: 1,
				layout: $state.snapshot(this.layout),
				focused: this.focusedId,
				tabs
			});
		} catch {
			// Disconnected mid-save; the next change saves again.
		}
	}

	// ------------------------------------------------------------ queries

	get paneCount(): number {
		return countPanes(this.layout);
	}

	get order(): string[] {
		return visualOrder(this.layout);
	}

	get focused(): PaneState {
		return this.panes[this.focusedId] ?? Object.values(this.panes)[0];
	}

	get canSplit(): boolean {
		return this.paneCount < MAX_PANES;
	}

	pane(id: string): PaneState | undefined {
		return this.panes[id];
	}

	// ------------------------------------------------------------ tabs

	focus(paneId: string): void {
		if (this.panes[paneId] && this.focusedId !== paneId) {
			this.focusedId = paneId;
			this.#save();
		}
	}

	/** Show a tab in a pane (the focused one by default). */
	openTab(tabId: TabId, paneId: string = this.focusedId): void {
		const p = this.panes[paneId];
		if (!p || !tabDef(tabId)) return;
		this.focusedId = paneId;
		if (p.tab === tabId) {
			p.instances.get(tabId)?.onReselect?.();
			return;
		}
		p.history.push(p.tab);
		if (p.history.length > 50) p.history.shift();
		if (!p.mounted.includes(tabId)) p.mounted.push(tabId);
		p.tab = tabId;
		this.#save();
	}

	/** Open a tab in a new pane next to the focused one, or in place if full. */
	openInNewPane(tabId: TabId, edge: Edge = 'right'): void {
		if (!this.canSplit) {
			this.openTab(tabId);
			return;
		}
		this.#addPane(this.focusedId, edge, tabId);
	}

	/** Send a request to a tab and bring that tab to the front. */
	request<K extends keyof TabRequests>(
		tabId: K & TabId,
		payload: TabRequests[K],
		options: { newPane?: boolean } = {}
	): void {
		this.requests = { ...this.requests, [tabId]: payload };
		const showing = this.order.find((id) => this.panes[id]?.tab === tabId);
		if (showing) this.focus(showing);
		else if (options.newPane) this.openInNewPane(tabId);
		else this.openTab(tabId);
	}

	/** Take (and clear) the pending request for a tab. */
	takeRequest<K extends keyof TabRequests>(tabId: K): TabRequests[K] | undefined {
		const payload = this.requests[tabId];
		if (payload !== undefined) {
			const rest = { ...this.requests };
			delete rest[tabId];
			this.requests = rest;
		}
		return payload as TabRequests[K] | undefined;
	}

	cycleTab(step: 1 | -1): void {
		const ids = TABS.map((t) => t.id);
		const index = ids.indexOf(this.focused.tab);
		this.openTab(ids[(index + step + ids.length) % ids.length]);
	}

	refreshActive(paneId: string = this.focusedId): void {
		const p = this.panes[paneId];
		void p?.instances.get(p.tab)?.refresh?.();
	}

	registerInstance(paneId: string, tabId: TabId, exports: TabExports | null): void {
		const p = this.panes[paneId];
		if (!p) return;
		if (exports) p.instances.set(tabId, exports);
		else p.instances.delete(tabId);
	}

	setLive(paneId: string, key: string, reason: string | null): void {
		const p = this.panes[paneId];
		if (!p) return;
		if (reason) p.live[key] = reason;
		else delete p.live[key];
	}

	// ------------------------------------------------------------ panes

	#addPane(targetId: string, edge: Edge, tab: TabId): string | null {
		if (!this.canSplit || !hasPane(this.layout, targetId)) return null;
		const id = uid();
		this.panes[id] = new PaneState(id, tab);
		this.layout = splitPane(this.layout, targetId, edge, id, uid());
		this.focusedId = id;
		this.#save();
		return id;
	}

	/** Split a pane; the new pane shows the same tab type as a fresh instance. */
	split(dir: 'vertical' | 'horizontal', paneId: string = this.focusedId): void {
		const source = this.panes[paneId];
		if (!source) return;
		this.#addPane(paneId, dir === 'vertical' ? 'right' : 'bottom', source.tab);
	}

	async close(paneId: string = this.focusedId): Promise<void> {
		if (this.paneCount <= 1 || !this.panes[paneId]) return;
		if (!(await this.#confirmDiscard([paneId], 'Close pane?'))) return;
		this.#drop([paneId]);
	}

	#drop(ids: string[]): void {
		let layout = this.layout;
		for (const id of ids) {
			layout = removePane(layout, id) ?? layout;
			delete this.panes[id];
		}
		this.backStack = this.backStack.filter((e) => e.paneId === null || !ids.includes(e.paneId));
		this.layout = layout;
		if (!this.panes[this.focusedId]) this.focusedId = visualOrder(layout)[0];
		this.#save();
	}

	async #confirmDiscard(ids: string[], title: string): Promise<boolean> {
		const reasons = ids.flatMap((id) => this.panes[id]?.liveReasons ?? []);
		if (reasons.length === 0) return true;
		return confirm({
			title,
			message: 'This will end what is running in the affected panes:',
			detail: [...new Set(reasons)].join('\n'),
			confirmLabel: 'Close anyway',
			destructive: true
		});
	}

	resize(splitId: string, ratio: number, min: number): void {
		this.layout = setRatio(this.layout, splitId, ratio, min);
		this.#save();
	}

	/** Apply a layout preset, keeping panes in reading order and adding or removing as needed. */
	async applyPreset(preset: Preset): Promise<void> {
		const need = presetSize(preset);
		const order = this.order;
		// Keep the focused pane when shrinking.
		const keep = order.slice(0, need);
		if (!keep.includes(this.focusedId) && need > 0) keep[keep.length - 1] = this.focusedId;
		const kept = order.filter((id) => keep.includes(id));
		const dropped = order.filter((id) => !keep.includes(id));
		if (!(await this.#confirmDiscard(dropped, 'Change layout?'))) return;

		for (const id of dropped) delete this.panes[id];
		while (kept.length < need) {
			const id = uid();
			this.panes[id] = new PaneState(id, this.focused?.tab ?? DEFAULT_TAB);
			kept.push(id);
		}
		this.backStack = this.backStack.filter((e) => e.paneId === null || !dropped.includes(e.paneId));
		this.layout = buildPreset(preset, kept, uid);
		if (!this.panes[this.focusedId]) this.focusedId = kept[0];
		this.#save();
	}

	focusByIndex(index: number): void {
		const id = this.order[index];
		if (id) this.focus(id);
	}

	/** A sidebar tab was dropped on a pane. */
	dropTab(tabId: TabId, targetId: string, zone: DropZone): void {
		if (zone === 'center' || !this.canSplit) this.openTab(tabId, targetId);
		else this.#addPane(targetId, zone, tabId);
	}

	/** A pane (dragged by its header) was dropped on another pane. */
	dropPane(sourceId: string, targetId: string, zone: DropZone): void {
		if (sourceId === targetId) return;
		this.layout =
			zone === 'center'
				? swapPanes(this.layout, sourceId, targetId)
				: movePane(this.layout, sourceId, targetId, zone, uid());
		this.focusedId = sourceId;
		this.#save();
	}

	// ------------------------------------------------------------ back navigation

	/**
	 * Register something Back should undo (close a modal, leave a sub-view…).
	 * Returns a function that removes the handler again.
	 */
	pushBack(label: string, run: () => void, paneId: string | null = null): () => void {
		const id = uid();
		this.backStack.push({ id, label, paneId, run });
		return () => {
			this.backStack = this.backStack.filter((e) => e.id !== id);
		};
	}

	#topBack(): BackEntry | undefined {
		for (let i = this.backStack.length - 1; i >= 0; i--) {
			const entry = this.backStack[i];
			if (entry.paneId === null || entry.paneId === this.focusedId) return entry;
		}
		return undefined;
	}

	/** What Back would do right now, for the button tooltip. `null` when nothing. */
	get backLabel(): string | null {
		const entry = this.#topBack();
		if (entry) return entry.label;
		const previous = this.focused?.history.at(-1);
		return previous ? `Back to ${tabDef(previous)?.label ?? previous}` : null;
	}

	back(): void {
		const entry = this.#topBack();
		if (entry) {
			entry.run();
			return;
		}
		const p = this.focused;
		const previous = p?.history.pop();
		if (p && previous) {
			if (!p.mounted.includes(previous)) p.mounted.push(previous);
			p.tab = previous;
			this.#save();
		}
	}
}

export const workspace = new Workspace();

// ---------------------------------------------------------------- pane context

/** What a tab can do with the pane it lives in. */
export interface PaneContext {
	readonly paneId: string;
	/** Mark the pane as holding live state so closing it asks first. */
	setLive(key: string, reason: string | null): void;
	/** Register a Back step scoped to this pane. */
	pushBack(label: string, run: () => void): () => void;
}

const PANE_CONTEXT = Symbol('pane');

export function providePaneContext(paneId: string): PaneContext {
	const context: PaneContext = {
		paneId,
		setLive: (key, reason) => workspace.setLive(paneId, key, reason),
		pushBack: (label, run) => workspace.pushBack(label, run, paneId)
	};
	setContext(PANE_CONTEXT, context);
	return context;
}

export function usePane(): PaneContext {
	return getContext<PaneContext>(PANE_CONTEXT);
}

/**
 * Keep a Back step registered while `active()` is true. Call during
 * component initialisation.
 */
export function backWhile(active: () => boolean, label: string, run: () => void): void {
	const pane = getContext<PaneContext | undefined>(PANE_CONTEXT);
	$effect(() => {
		if (!active()) return;
		return pane ? pane.pushBack(label, run) : workspace.pushBack(label, run);
	});
}
