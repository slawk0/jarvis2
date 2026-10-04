import { describe, expect, it } from 'vitest';
import {
	MAX_PANES,
	buildPreset,
	computeDividers,
	computeRects,
	countPanes,
	dropZoneAt,
	movePane,
	pane,
	paneIds,
	parseLayout,
	removePane,
	setRatio,
	splitPane,
	swapPanes,
	visualOrder,
	type LayoutNode,
	type Rect
} from './layout';

let counter = 0;
const newId = () => `s${++counter}`;

function area(rects: Map<string, Rect>): number {
	return [...rects.values()].reduce((sum, r) => sum + r.w * r.h, 0);
}

/** No two panes overlap and together they cover the whole workspace. */
function expectTiling(root: LayoutNode) {
	const rects = computeRects(root);
	expect(area(rects)).toBeCloseTo(1, 9);
	const list = [...rects.values()];
	for (let i = 0; i < list.length; i++) {
		for (let j = i + 1; j < list.length; j++) {
			const a = list[i];
			const b = list[j];
			const overlapX = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
			const overlapY = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
			expect(overlapX > 1e-9 && overlapY > 1e-9).toBe(false);
		}
	}
}

describe('splitPane', () => {
	it('splits right and bottom by default halves', () => {
		let root: LayoutNode = pane('a');
		root = splitPane(root, 'a', 'right', 'b', newId());
		expect(computeRects(root).get('a')).toEqual({ x: 0, y: 0, w: 0.5, h: 1 });
		expect(computeRects(root).get('b')).toEqual({ x: 0.5, y: 0, w: 0.5, h: 1 });
		root = splitPane(root, 'b', 'bottom', 'c', newId());
		expect(computeRects(root).get('c')).toEqual({ x: 0.5, y: 0.5, w: 0.5, h: 0.5 });
		expectTiling(root);
	});

	it('places the new pane first for left/top', () => {
		const root = splitPane(pane('a'), 'a', 'left', 'b', newId());
		expect(visualOrder(root)).toEqual(['b', 'a']);
		const stacked = splitPane(pane('a'), 'a', 'top', 'b', newId());
		expect(visualOrder(stacked)).toEqual(['b', 'a']);
	});

	it('refuses to exceed the pane limit', () => {
		let root: LayoutNode = pane('p0');
		for (let i = 1; i < MAX_PANES; i++) root = splitPane(root, 'p0', 'right', `p${i}`, newId());
		expect(countPanes(root)).toBe(MAX_PANES);
		const same = splitPane(root, 'p0', 'right', 'extra', newId());
		expect(same).toBe(root);
	});

	it('ignores unknown targets and duplicate ids', () => {
		const root = splitPane(pane('a'), 'a', 'right', 'b', newId());
		expect(splitPane(root, 'zzz', 'right', 'c', newId())).toBe(root);
		expect(splitPane(root, 'a', 'right', 'b', newId())).toBe(root);
	});
});

describe('removePane', () => {
	it('lets the sibling absorb the space, leaving no hole', () => {
		const grid = buildPreset('grid', ['a', 'b', 'c', 'd'], newId);
		for (const id of ['a', 'b', 'c', 'd']) {
			const next = removePane(grid, id)!;
			expect(countPanes(next)).toBe(3);
			expectTiling(next);
		}
	});

	it('returns null for the last pane and is a no-op for unknown ids', () => {
		expect(removePane(pane('a'), 'a')).toBeNull();
		const root = pane('a');
		expect(removePane(root, 'x')).toBe(root);
	});

	it('collapses nested splits', () => {
		const grid = buildPreset('grid', ['a', 'b', 'c', 'd'], newId);
		const two = removePane(removePane(grid, 'a')!, 'b')!;
		expect(paneIds(two)).toEqual(['c', 'd']);
		expect(computeRects(two).get('c')).toEqual({ x: 0, y: 0, w: 0.5, h: 1 });
	});
});

describe('setRatio', () => {
	it('resizes one split and clamps to the minimum pane size', () => {
		const root = buildPreset('columns', ['a', 'b'], () => 'main');
		expect(computeRects(setRatio(root, 'main', 0.3)).get('a')!.w).toBeCloseTo(0.3);
		expect(computeRects(setRatio(root, 'main', 0.01, 0.2)).get('a')!.w).toBeCloseTo(0.2);
		expect(computeRects(setRatio(root, 'main', 0.99, 0.2)).get('a')!.w).toBeCloseTo(0.8);
		expect(setRatio(root, 'nope', 0.3)).toBe(root);
	});
});

describe('movePane and swapPanes', () => {
	it('moves a pane next to another without losing any', () => {
		const grid = buildPreset('grid', ['a', 'b', 'c', 'd'], newId);
		const moved = movePane(grid, 'a', 'd', 'right', newId());
		expect(paneIds(moved).sort()).toEqual(['a', 'b', 'c', 'd']);
		expectTiling(moved);
		const ra = computeRects(moved).get('a')!;
		const rd = computeRects(moved).get('d')!;
		expect(ra.x).toBeCloseTo(rd.x + rd.w);
		expect(ra.y).toBeCloseTo(rd.y);
	});

	it('is a no-op when moving onto itself', () => {
		const root = buildPreset('columns', ['a', 'b'], newId);
		expect(movePane(root, 'a', 'a', 'left', newId())).toBe(root);
	});

	it('swaps positions', () => {
		const root = buildPreset('columns', ['a', 'b'], newId);
		expect(visualOrder(swapPanes(root, 'a', 'b'))).toEqual(['b', 'a']);
		expect(swapPanes(root, 'a', 'a')).toBe(root);
	});
});

describe('presets and ordering', () => {
	it('builds each preset as a full tiling', () => {
		expectTiling(buildPreset('single', ['a'], newId));
		expectTiling(buildPreset('columns', ['a', 'b'], newId));
		expectTiling(buildPreset('rows', ['a', 'b'], newId));
		expectTiling(buildPreset('grid', ['a', 'b', 'c', 'd'], newId));
		expect(() => buildPreset('grid', ['a'], newId)).toThrow();
	});

	it('visual order is reading order', () => {
		const grid = buildPreset('grid', ['a', 'b', 'c', 'd'], newId);
		expect(visualOrder(grid)).toEqual(['a', 'b', 'c', 'd']);
		expect(visualOrder(buildPreset('rows', ['top', 'bottom'], newId))).toEqual(['top', 'bottom']);
	});

	it('reports one divider per split', () => {
		const grid = buildPreset('grid', ['a', 'b', 'c', 'd'], newId);
		const dividers = computeDividers(grid);
		expect(dividers).toHaveLength(3);
		expect(dividers[0]).toMatchObject({ dir: 'col', rect: { x: 0, y: 0.5, w: 1, h: 0 } });
		expect(dividers[1]).toMatchObject({ dir: 'row', rect: { x: 0.5, y: 0, w: 0, h: 0.5 } });
	});
});

describe('dropZoneAt', () => {
	it('detects edges and centre', () => {
		expect(dropZoneAt(0.5, 0.5)).toBe('center');
		expect(dropZoneAt(0.05, 0.5)).toBe('left');
		expect(dropZoneAt(0.95, 0.5)).toBe('right');
		expect(dropZoneAt(0.5, 0.1)).toBe('top');
		expect(dropZoneAt(0.5, 0.9)).toBe('bottom');
	});
});

describe('parseLayout', () => {
	it('round-trips a valid tree through JSON', () => {
		const grid = buildPreset('grid', ['a', 'b', 'c', 'd'], newId);
		expect(parseLayout(JSON.parse(JSON.stringify(grid)))).toEqual(grid);
	});

	it('rejects malformed input', () => {
		expect(parseLayout(null)).toBeNull();
		expect(parseLayout({ kind: 'pane' })).toBeNull();
		expect(
			parseLayout({ kind: 'split', id: 's', dir: 'row', ratio: 2, a: pane('a'), b: pane('b') })
		).toBeNull();
		expect(
			parseLayout({ kind: 'split', id: 's', dir: 'row', ratio: 0.5, a: pane('a'), b: pane('a') })
		).toBeNull();
		const five = splitPane(buildPreset('grid', ['a', 'b', 'c', 'd'], newId), 'a', 'left', 'e', 'x');
		expect(countPanes(five)).toBe(4);
	});
});
