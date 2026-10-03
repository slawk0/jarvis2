import { describe, expect, it } from 'vitest';
import {
	formatBytes,
	formatClock,
	formatDuration,
	formatMode,
	formatOctal,
	formatPercent,
	formatRelative,
	formatSpeed
} from './format';

describe('formatBytes', () => {
	it('scales through units', () => {
		expect(formatBytes(0)).toBe('0 B');
		expect(formatBytes(1023)).toBe('1023 B');
		expect(formatBytes(1024)).toBe('1.0 KB');
		expect(formatBytes(1536)).toBe('1.5 KB');
		expect(formatBytes(1024 ** 2 * 250)).toBe('250 MB');
		expect(formatBytes(1024 ** 3 * 2.5)).toBe('2.5 GB');
		expect(formatBytes(1024 ** 4)).toBe('1.0 TB');
	});

	it('handles missing and negative values', () => {
		expect(formatBytes(null)).toBe('—');
		expect(formatBytes(undefined)).toBe('—');
		expect(formatBytes(NaN)).toBe('—');
		expect(formatBytes(-2048)).toBe('-2.0 KB');
	});
});

describe('formatSpeed / formatPercent', () => {
	it('formats', () => {
		expect(formatSpeed(2048)).toBe('2.0 KB/s');
		expect(formatSpeed(null)).toBe('—');
		expect(formatPercent(42.4)).toBe('42%');
		expect(formatPercent(42.44, 1)).toBe('42.4%');
		expect(formatPercent(null)).toBe('—');
	});
});

describe('durations', () => {
	it('formatDuration keeps the significant parts', () => {
		expect(formatDuration(5)).toBe('5s');
		expect(formatDuration(65)).toBe('1m 5s');
		expect(formatDuration(3700)).toBe('1h 1m');
		expect(formatDuration(93784)).toBe('1d 2h 3m');
		expect(formatDuration(null)).toBe('—');
	});

	it('formatClock', () => {
		expect(formatClock(7000)).toBe('0:07');
		expect(formatClock(760_000)).toBe('12:40');
		expect(formatClock(3_723_000)).toBe('1:02:03');
	});
});

describe('formatRelative', () => {
	const now = Date.UTC(2026, 0, 15, 12, 0, 0);
	it('past and future', () => {
		expect(formatRelative(now - 2000, now)).toBe('just now');
		expect(formatRelative(now - 180_000, now)).toBe('3 minutes ago');
		expect(formatRelative(now - 3_600_000, now)).toBe('1 hour ago');
		expect(formatRelative(now + 2 * 86_400_000, now)).toBe('in 2 days');
		expect(formatRelative(null, now)).toBe('—');
	});

	it('treats small numbers as unix seconds', () => {
		expect(formatRelative(now / 1000 - 120, now)).toBe('2 minutes ago');
	});
});

describe('permissions', () => {
	it('symbolic and octal', () => {
		expect(formatMode(0o755)).toBe('rwxr-xr-x');
		expect(formatMode(0o640)).toBe('rw-r-----');
		expect(formatMode(0o100644)).toBe('rw-r--r--');
		expect(formatOctal(0o100644)).toBe('644');
		expect(formatOctal(0o4755)).toBe('4755');
	});
});
