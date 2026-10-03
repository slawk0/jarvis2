/** The one implementation of byte, speed, duration and date formatting. */

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];

export function formatBytes(bytes: number | null | undefined, digits = 1): string {
	if (bytes == null || !Number.isFinite(bytes)) return '—';
	const sign = bytes < 0 ? '-' : '';
	let value = Math.abs(bytes);
	let unit = 0;
	while (value >= 1024 && unit < UNITS.length - 1) {
		value /= 1024;
		unit++;
	}
	const shown = unit === 0 ? String(Math.round(value)) : value.toFixed(value >= 100 ? 0 : digits);
	return `${sign}${shown} ${UNITS[unit]}`;
}

export function formatSpeed(bytesPerSecond: number | null | undefined): string {
	if (bytesPerSecond == null || !Number.isFinite(bytesPerSecond)) return '—';
	return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatPercent(value: number | null | undefined, digits = 0): string {
	if (value == null || !Number.isFinite(value)) return '—';
	return `${value.toFixed(digits)}%`;
}

/** 93784 → "1d 2h 3m"; keeps the two or three most significant parts. */
export function formatDuration(totalSeconds: number | null | undefined): string {
	if (totalSeconds == null || !Number.isFinite(totalSeconds)) return '—';
	let s = Math.max(0, Math.round(totalSeconds));
	const d = Math.floor(s / 86400);
	s -= d * 86400;
	const h = Math.floor(s / 3600);
	s -= h * 3600;
	const m = Math.floor(s / 60);
	s -= m * 60;
	if (d > 0) return `${d}d ${h}h ${m}m`;
	if (h > 0) return `${h}h ${m}m`;
	if (m > 0) return `${m}m ${s}s`;
	return `${s}s`;
}

/** Elapsed time for job rows: "0:07", "12:40", "1:02:03". */
export function formatClock(ms: number): string {
	const total = Math.max(0, Math.floor(ms / 1000));
	const h = Math.floor(total / 3600);
	const m = Math.floor((total % 3600) / 60);
	const s = total % 60;
	const pad = (n: number) => String(n).padStart(2, '0');
	return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

function toDate(value: Date | number | string | null | undefined): Date | null {
	if (value == null || value === '') return null;
	// Numbers below ~year 2001 in milliseconds are Unix seconds.
	const date =
		typeof value === 'number' ? new Date(value < 1e12 ? value * 1000 : value) : new Date(value);
	return Number.isNaN(date.getTime()) ? null : date;
}

const DATE_TIME = new Intl.DateTimeFormat(undefined, {
	year: 'numeric',
	month: 'short',
	day: '2-digit',
	hour: '2-digit',
	minute: '2-digit'
});
const DATE_TIME_SECONDS = new Intl.DateTimeFormat(undefined, {
	year: 'numeric',
	month: 'short',
	day: '2-digit',
	hour: '2-digit',
	minute: '2-digit',
	second: '2-digit'
});
const DATE_ONLY = new Intl.DateTimeFormat(undefined, {
	year: 'numeric',
	month: 'short',
	day: '2-digit'
});

export function formatDateTime(
	value: Date | number | string | null | undefined,
	seconds = false
): string {
	const date = toDate(value);
	if (!date) return '—';
	return (seconds ? DATE_TIME_SECONDS : DATE_TIME).format(date);
}

export function formatDate(value: Date | number | string | null | undefined): string {
	const date = toDate(value);
	return date ? DATE_ONLY.format(date) : '—';
}

/** "3 minutes ago", "in 2 days". `now` is injectable for tests. */
export function formatRelative(
	value: Date | number | string | null | undefined,
	now: number = Date.now()
): string {
	const date = toDate(value);
	if (!date) return '—';
	const diff = Math.round((date.getTime() - now) / 1000);
	const abs = Math.abs(diff);
	const steps: [number, number, string][] = [
		[60, 1, 'second'],
		[3600, 60, 'minute'],
		[86400, 3600, 'hour'],
		[86400 * 30, 86400, 'day'],
		[86400 * 365, 86400 * 30, 'month'],
		[Infinity, 86400 * 365, 'year']
	];
	if (abs < 5) return 'just now';
	for (const [limit, size, unit] of steps) {
		if (abs < limit) {
			const n = Math.floor(abs / size);
			const label = `${n} ${unit}${n === 1 ? '' : 's'}`;
			return diff < 0 ? `${label} ago` : `in ${label}`;
		}
	}
	return '—';
}

export function formatNumber(value: number | null | undefined): string {
	if (value == null || !Number.isFinite(value)) return '—';
	return value.toLocaleString();
}

/** Unix permission bits → "rwxr-xr-x". */
export function formatMode(mode: number): string {
	const bits = ['r', 'w', 'x'];
	let out = '';
	for (let shift = 8; shift >= 0; shift--) {
		out += mode & (1 << shift) ? bits[(8 - shift) % 3] : '-';
	}
	return out;
}

export function formatOctal(mode: number): string {
	return (mode & 0o7777).toString(8).padStart(3, '0');
}
