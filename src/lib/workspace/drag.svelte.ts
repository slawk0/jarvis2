/**
 * Pointer-based dragging of sidebar tabs and panes onto panes.
 *
 * HTML5 drag-and-drop is not used: the native file-drop handler (needed for
 * uploads from the OS) swallows it on Windows.
 */
import { computeRects, dropZoneAt, type DropZone } from './layout';
import type { TabId } from './registry';
import { workspace } from './workspace.svelte';

export type DragPayload = { kind: 'tab'; tabId: TabId } | { kind: 'pane'; paneId: string };

const THRESHOLD = 6;

class DragController {
	payload = $state<DragPayload | null>(null);
	label = $state('');
	x = $state(0);
	y = $state(0);
	target = $state<{ paneId: string; zone: DropZone } | null>(null);
	/** The pane grid element, used to map pointer positions to panes. */
	grid: HTMLElement | null = null;

	/** Call from `pointerdown`; the drag starts once the pointer moves a little. */
	begin(event: PointerEvent, payload: DragPayload, label: string): void {
		if (event.button !== 0) return;
		const startX = event.clientX;
		const startY = event.clientY;
		let started = false;

		const move = (e: PointerEvent) => {
			if (!started) {
				if (Math.hypot(e.clientX - startX, e.clientY - startY) < THRESHOLD) return;
				started = true;
				this.payload = payload;
				this.label = label;
				document.body.style.cursor = 'grabbing';
			}
			this.x = e.clientX;
			this.y = e.clientY;
			this.target = this.#hit(e.clientX, e.clientY, payload);
		};

		const finish = (commit: boolean) => {
			window.removeEventListener('pointermove', move);
			window.removeEventListener('pointerup', up);
			window.removeEventListener('keydown', key, true);
			document.body.style.cursor = '';
			const target = this.target;
			this.payload = null;
			this.target = null;
			if (!started || !commit || !target) return;
			if (payload.kind === 'tab') workspace.dropTab(payload.tabId, target.paneId, target.zone);
			else workspace.dropPane(payload.paneId, target.paneId, target.zone);
		};
		const up = () => finish(true);
		const key = (e: KeyboardEvent) => {
			if (e.key === 'Escape') {
				e.stopPropagation();
				finish(false);
			}
		};

		window.addEventListener('pointermove', move);
		window.addEventListener('pointerup', up);
		window.addEventListener('keydown', key, true);
	}

	/** True right after a drag, so the click that ends it can be ignored. */
	get dragging(): boolean {
		return this.payload !== null;
	}

	#hit(x: number, y: number, payload: DragPayload): { paneId: string; zone: DropZone } | null {
		const bounds = this.grid?.getBoundingClientRect();
		if (!bounds || bounds.width === 0 || bounds.height === 0) return null;
		const fx = (x - bounds.left) / bounds.width;
		const fy = (y - bounds.top) / bounds.height;
		if (fx < 0 || fx > 1 || fy < 0 || fy > 1) return null;
		for (const [paneId, r] of computeRects(workspace.layout)) {
			if (fx < r.x || fx > r.x + r.w || fy < r.y || fy > r.y + r.h) continue;
			if (payload.kind === 'pane' && payload.paneId === paneId) return null;
			let zone = dropZoneAt((fx - r.x) / r.w, (fy - r.y) / r.h);
			// A new pane cannot be created when the limit is reached.
			if (payload.kind === 'tab' && !workspace.canSplit) zone = 'center';
			return { paneId, zone };
		}
		return null;
	}
}

export const drag = new DragController();
