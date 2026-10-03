/**
 * Pane layout engine: a binary split tree. Pure functions, no DOM.
 *
 * A tree makes the hard guarantees trivial: closing a pane hands its space to
 * its sibling (never a hole), and every divider belongs to exactly one split.
 * Panes are rendered flat and positioned from `computeRects`, so restructuring
 * the tree never remounts a pane's content.
 */

export const MAX_PANES = 4;

/** `row` places children side by side; `col` stacks them. */
export type SplitDir = 'row' | 'col';
export type Edge = 'left' | 'right' | 'top' | 'bottom';
export type DropZone = Edge | 'center';
export type Preset = 'single' | 'columns' | 'rows' | 'grid';

export interface PaneNode {
	kind: 'pane';
	id: string;
}

export interface SplitNode {
	kind: 'split';
	id: string;
	dir: SplitDir;
	/** Share of the space given to `a`, between 0 and 1. */
	ratio: number;
	a: LayoutNode;
	b: LayoutNode;
}

export type LayoutNode = PaneNode | SplitNode;

/** Fractions of the workspace area. */
export interface Rect {
	x: number;
	y: number;
	w: number;
	h: number;
}

export interface Divider {
	splitId: string;
	dir: SplitDir;
	/** The divider line, as a rect with zero thickness along its split axis. */
	rect: Rect;
	/** Area of the split this divider belongs to (for converting drags to ratios). */
	area: Rect;
}

const FULL: Rect = { x: 0, y: 0, w: 1, h: 1 };

export const pane = (id: string): PaneNode => ({ kind: 'pane', id });

function splitNode(
	id: string,
	dir: SplitDir,
	a: LayoutNode,
	b: LayoutNode,
	ratio = 0.5
): SplitNode {
	return { kind: 'split', id, dir, ratio, a, b };
}

export function paneIds(node: LayoutNode): string[] {
	return node.kind === 'pane' ? [node.id] : [...paneIds(node.a), ...paneIds(node.b)];
}

export function countPanes(node: LayoutNode): number {
	return paneIds(node).length;
}

export function hasPane(node: LayoutNode, id: string): boolean {
	return paneIds(node).includes(id);
}

function childAreas(split: SplitNode, area: Rect): [Rect, Rect] {
	if (split.dir === 'row') {
		const wa = area.w * split.ratio;
		return [
			{ x: area.x, y: area.y, w: wa, h: area.h },
			{ x: area.x + wa, y: area.y, w: area.w - wa, h: area.h }
		];
	}
	const ha = area.h * split.ratio;
	return [
		{ x: area.x, y: area.y, w: area.w, h: ha },
		{ x: area.x, y: area.y + ha, w: area.w, h: area.h - ha }
	];
}

export function computeRects(node: LayoutNode, area: Rect = FULL): Map<string, Rect> {
	const rects = new Map<string, Rect>();
	const walk = (n: LayoutNode, r: Rect) => {
		if (n.kind === 'pane') {
			rects.set(n.id, r);
			return;
		}
		const [ra, rb] = childAreas(n, r);
		walk(n.a, ra);
		walk(n.b, rb);
	};
	walk(node, area);
	return rects;
}

export function computeDividers(node: LayoutNode, area: Rect = FULL): Divider[] {
	if (node.kind === 'pane') return [];
	const [ra, rb] = childAreas(node, area);
	const rect: Rect =
		node.dir === 'row'
			? { x: rb.x, y: area.y, w: 0, h: area.h }
			: { x: area.x, y: rb.y, w: area.w, h: 0 };
	return [
		{ splitId: node.id, dir: node.dir, rect, area },
		...computeDividers(node.a, ra),
		...computeDividers(node.b, rb)
	];
}

/** Pane ids in reading order (top to bottom, then left to right). */
export function visualOrder(node: LayoutNode): string[] {
	const rects = computeRects(node);
	const eps = 1e-6;
	return [...rects.entries()]
		.sort(([, a], [, b]) => (Math.abs(a.y - b.y) > eps ? a.y - b.y : a.x - b.x))
		.map(([id]) => id);
}

function replace(
	node: LayoutNode,
	id: string,
	make: (found: PaneNode) => LayoutNode
): LayoutNode {
	if (node.kind === 'pane') return node.id === id ? make(node) : node;
	const a = replace(node.a, id, make);
	const b = replace(node.b, id, make);
	return a === node.a && b === node.b ? node : { ...node, a, b };
}

/**
 * Put a new pane on one edge of an existing pane. Returns the tree unchanged
 * when the target is missing or the pane limit is reached.
 */
export function splitPane(
	root: LayoutNode,
	targetId: string,
	edge: Edge,
	newPaneId: string,
	splitId: string
): LayoutNode {
	if (countPanes(root) >= MAX_PANES || !hasPane(root, targetId) || hasPane(root, newPaneId)) {
		return root;
	}
	const dir: SplitDir = edge === 'left' || edge === 'right' ? 'row' : 'col';
	const first = edge === 'left' || edge === 'top';
	return replace(root, targetId, (target) =>
		first
			? splitNode(splitId, dir, pane(newPaneId), target)
			: splitNode(splitId, dir, target, pane(newPaneId))
	);
}

/** Remove a pane; its sibling absorbs the space. `null` when it was the last one. */
export function removePane(root: LayoutNode, id: string): LayoutNode | null {
	if (root.kind === 'pane') return root.id === id ? null : root;
	const a = removePane(root.a, id);
	const b = removePane(root.b, id);
	if (a === null) return b;
	if (b === null) return a;
	return a === root.a && b === root.b ? root : { ...root, a, b };
}

/** Set a split's ratio, keeping both sides at least `min` of the split. */
export function setRatio(root: LayoutNode, splitId: string, ratio: number, min = 0.15): LayoutNode {
	if (root.kind === 'pane') return root;
	if (root.id === splitId) {
		const clamped = Math.min(1 - min, Math.max(min, ratio));
		return clamped === root.ratio ? root : { ...root, ratio: clamped };
	}
	const a = setRatio(root.a, splitId, ratio, min);
	const b = setRatio(root.b, splitId, ratio, min);
	return a === root.a && b === root.b ? root : { ...root, a, b };
}

/** Move an existing pane to an edge of another pane. */
export function movePane(
	root: LayoutNode,
	id: string,
	targetId: string,
	edge: Edge,
	splitId: string
): LayoutNode {
	if (id === targetId || !hasPane(root, id) || !hasPane(root, targetId)) return root;
	const without = removePane(root, id);
	if (!without) return root;
	const dir: SplitDir = edge === 'left' || edge === 'right' ? 'row' : 'col';
	const first = edge === 'left' || edge === 'top';
	return replace(without, targetId, (target) =>
		first ? splitNode(splitId, dir, pane(id), target) : splitNode(splitId, dir, target, pane(id))
	);
}

/** Exchange the positions of two panes. */
export function swapPanes(root: LayoutNode, a: string, b: string): LayoutNode {
	if (a === b || !hasPane(root, a) || !hasPane(root, b)) return root;
	const swap = (node: LayoutNode): LayoutNode => {
		if (node.kind === 'pane') {
			if (node.id === a) return pane(b);
			if (node.id === b) return pane(a);
			return node;
		}
		return { ...node, a: swap(node.a), b: swap(node.b) };
	};
	return swap(root);
}

export function presetSize(preset: Preset): number {
	return { single: 1, columns: 2, rows: 2, grid: 4 }[preset];
}

/**
 * Build a preset layout from exactly `presetSize(preset)` pane ids, given in
 * the order they should appear (reading order).
 */
export function buildPreset(preset: Preset, ids: string[], newId: () => string): LayoutNode {
	const need = presetSize(preset);
	if (ids.length !== need) throw new Error(`preset ${preset} needs ${need} panes`);
	switch (preset) {
		case 'single':
			return pane(ids[0]);
		case 'columns':
			return splitNode(newId(), 'row', pane(ids[0]), pane(ids[1]));
		case 'rows':
			return splitNode(newId(), 'col', pane(ids[0]), pane(ids[1]));
		case 'grid':
			return splitNode(
				newId(),
				'col',
				splitNode(newId(), 'row', pane(ids[0]), pane(ids[1])),
				splitNode(newId(), 'row', pane(ids[2]), pane(ids[3]))
			);
	}
}

/** Which zone of a pane a pointer is over: an edge band or the centre. */
export function dropZoneAt(fx: number, fy: number, band = 0.25): DropZone {
	const distances: [Edge, number][] = [
		['left', fx],
		['right', 1 - fx],
		['top', fy],
		['bottom', 1 - fy]
	];
	const [edge, distance] = distances.reduce((best, d) => (d[1] < best[1] ? d : best));
	return distance < band ? edge : 'center';
}

/** Validate an untrusted (persisted) tree. Returns null if it is not usable. */
export function parseLayout(value: unknown): LayoutNode | null {
	const seen = new Set<string>();
	const walk = (v: unknown): LayoutNode | null => {
		if (typeof v !== 'object' || v === null) return null;
		const n = v as Record<string, unknown>;
		if (n.kind === 'pane' && typeof n.id === 'string' && !seen.has(n.id)) {
			seen.add(n.id);
			return pane(n.id);
		}
		if (
			n.kind === 'split' &&
			typeof n.id === 'string' &&
			(n.dir === 'row' || n.dir === 'col') &&
			typeof n.ratio === 'number' &&
			n.ratio > 0 &&
			n.ratio < 1
		) {
			const a = walk(n.a);
			const b = walk(n.b);
			if (a && b) return { kind: 'split', id: n.id, dir: n.dir, ratio: n.ratio, a, b };
		}
		return null;
	};
	const root = walk(value);
	return root && seen.size >= 1 && seen.size <= MAX_PANES ? root : null;
}
