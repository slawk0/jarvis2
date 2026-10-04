/**
 * Thin typed helpers over the backend Pangolin proxy. Responses are parsed
 * defensively: field names differ slightly between Pangolin versions.
 */
import { api, type HttpMethod } from '$lib/ipc';
import { settings } from '$lib/services/settings.svelte';

export type Json = Record<string, unknown>;

export async function pangolin<T = Json>(
	method: HttpMethod,
	path: string,
	options: { query?: Record<string, string | number | undefined | null>; body?: unknown } = {}
): Promise<T> {
	const query = Object.entries(options.query ?? {})
		.filter(([, v]) => v !== undefined && v !== null && v !== '')
		.map(([k, v]) => [k, String(v)] as [string, string]);
	const body = options.body === undefined ? null : JSON.stringify(options.body);
	const text = await api.pangolinRequest(method, path, query, body);
	if (!text.trim()) return {} as T;
	const parsed = JSON.parse(text) as Json;
	// Pangolin wraps payloads as { data, success, message }.
	return ('data' in parsed && parsed.data !== null ? parsed.data : parsed) as T;
}

export function orgPath(suffix: string): string {
	return `/v1/org/${settings.value.pangolin.orgId}${suffix}`;
}

/** First array found under any of the given keys (or the value itself if it is an array). */
export function list(value: unknown, ...keys: string[]): Json[] {
	if (Array.isArray(value)) return value as Json[];
	if (value && typeof value === 'object') {
		for (const key of keys) {
			const candidate = (value as Json)[key];
			if (Array.isArray(candidate)) return candidate as Json[];
		}
	}
	return [];
}

export const str = (v: unknown): string => (v == null ? '' : String(v));
export const num = (v: unknown): number => (typeof v === 'number' ? v : Number(v) || 0);
export const pick = (o: Json, ...keys: string[]): unknown => keys.map((k) => o[k]).find((v) => v != null);

export interface AnalyticsFilters {
	resourceId?: string;
	action?: string;
	location?: string;
	host?: string;
}

export interface Analytics {
	total: number;
	allowed: number;
	blocked: number;
	blockRate: number;
	countries: { code: string; count: number; blocked: number }[];
	timeline: { label: string; total: number; blocked: number }[];
}

export async function fetchAnalytics(hours: number, filters: AnalyticsFilters = {}): Promise<Analytics> {
	const end = new Date();
	const start = new Date(end.getTime() - hours * 3_600_000);
	// The analytics endpoint only filters by resource; other filters go through `analyticsFromLogs`.
	const data = await pangolin('GET', orgPath('/logs/analytics'), {
		query: { timeStart: start.toISOString(), timeEnd: end.toISOString(), resourceId: filters.resourceId }
	});
	const total = num(pick(data, 'totalRequests', 'total'));
	const blocked = num(pick(data, 'totalBlocked', 'blocked'));
	const allowed =
		pick(data, 'totalAllowed', 'allowed') != null
			? num(pick(data, 'totalAllowed', 'allowed'))
			: total - blocked;
	const countries = list(pick(data, 'requestsPerCountry', 'countries'))
		.map((c) => ({
			code: str(pick(c, 'code', 'country_code', 'countryCode', 'country', 'location')).toUpperCase(),
			count: num(pick(c, 'count', 'totalRequests', 'total')),
			blocked: num(pick(c, 'blockedCount', 'blocked'))
		}))
		.filter((c) => c.code)
		.sort((a, b) => b.count - a.count);
	const timeline = list(pick(data, 'requestsPerDay', 'requestsPerHour', 'timeline')).map((p) => ({
		label: str(pick(p, 'day', 'hour', 'time', 'date')),
		total: num(pick(p, 'totalCount', 'totalRequests', 'total', 'count')),
		blocked: num(pick(p, 'blockedCount', 'totalBlocked', 'blocked'))
	}));
	return { total, allowed, blocked, blockRate: total > 0 ? (blocked / total) * 100 : 0, countries, timeline };
}

/**
 * Fetch every item of a list endpoint. Pangolin pages either with
 * `page`/`pageSize` or with `limit`/`offset`; both are sent and paging stops
 * when a page comes back short or repeats.
 */
export async function listAll(
	path: string,
	keys: string[],
	query: Record<string, string | number | undefined> = {}
): Promise<Json[]> {
	const PAGE = 100;
	const MAX_PAGES = 50;
	const all: Json[] = [];
	let firstId = '';
	for (let page = 1; page <= MAX_PAGES; page++) {
		const data = await pangolin('GET', path, {
			query: { ...query, pageSize: PAGE, page, limit: PAGE, offset: (page - 1) * PAGE }
		});
		const items = list(data, ...keys);
		const id = JSON.stringify(items[0] ?? null);
		// A server that ignores paging returns the same first page again.
		if (page > 1 && id === firstId) break;
		firstId = page === 1 ? id : firstId;
		all.push(...items);
		if (items.length < PAGE) break;
	}
	return all;
}

export const bool = (v: unknown): boolean => v === true || v === 1 || v === 'true';

/** Seconds or milliseconds since the epoch, or an ISO string → Date (null when empty). */
export function toDate(v: unknown): Date | null {
	if (v == null || v === '' || v === 0) return null;
	if (typeof v === 'number') return new Date(v < 1e12 ? v * 1000 : v);
	const text = String(v);
	if (/^\d+$/.test(text)) return toDate(Number(text));
	const date = new Date(text);
	return Number.isNaN(date.getTime()) ? null : date;
}

export interface LogEntry {
	id: string;
	time: Date | null;
	allowed: boolean;
	ip: string;
	location: string;
	resource: string;
	host: string;
	path: string;
	method: string;
	reason: string;
	actor: string;
	raw: Json;
}

export interface LogFilters {
	action?: string;
	location?: string;
	resourceId?: string;
	host?: string;
	path?: string;
	method?: string;
	reason?: string;
	actor?: string;
}

/** Why Pangolin allowed or blocked a request (codes from its request audit log). */
const REASONS: Record<string, string> = {
	'100': 'Allowed by rule',
	'101': 'No authentication required',
	'102': 'Valid access token',
	'103': 'Valid header auth',
	'104': 'Valid PIN code',
	'105': 'Valid password',
	'106': 'Valid email',
	'107': 'Valid SSO session',
	'201': 'Resource not found',
	'202': 'Blocked by rule',
	'203': 'No session',
	'204': 'Temporary request token',
	'205': 'No more auth methods',
	'206': 'Country blocked'
};

export function reasonLabel(value: unknown): string {
	const code = str(value);
	return REASONS[code] ? `${REASONS[code]} (${code})` : code;
}

function toLogEntry(row: Json, index: number): LogEntry {
	const action = pick(row, 'action', 'allowed');
	return {
		id: str(pick(row, 'id', 'requestId')) || `${str(row.timestamp)}-${index}`,
		time: toDate(pick(row, 'timestamp', 'time', 'createdAt')),
		allowed: bool(action) || action === 'allow' || action === 'allowed',
		ip: str(pick(row, 'ip', 'clientIp')).replace(/:\d+$/, (m) =>
			str(pick(row, 'ip', 'clientIp')).includes('.') ? '' : m
		),
		location: str(pick(row, 'location', 'country')).toUpperCase(),
		resource: str(pick(row, 'resourceName', 'resourceNiceId', 'resourceId')),
		host: str(row.host),
		path: str(row.path),
		method: str(row.method),
		reason: reasonLabel(row.reason),
		actor: str(pick(row, 'actor', 'actorId', 'actorType')),
		raw: row
	};
}

export interface LogPage {
	entries: LogEntry[];
	/** Total matching rows when the server reports it. */
	total: number | null;
}

/** One page of the request audit log, newest first. */
export async function fetchLogs(
	start: Date,
	end: Date,
	filters: LogFilters,
	limit: number,
	offset: number
): Promise<LogPage> {
	const data = await pangolin('GET', orgPath('/logs/request'), {
		query: { timeStart: start.toISOString(), timeEnd: end.toISOString(), ...filters, limit, offset }
	});
	const rows = list(data, 'log', 'logs', 'requests');
	const pagination = (data.pagination ?? {}) as Json;
	const total = pagination.total != null ? num(pagination.total) : null;
	// A server without paging support returns everything: page locally.
	const paged = rows.length > limit ? rows.slice(offset, offset + limit) : rows;
	return { entries: paged.map(toLogEntry), total: rows.length > limit ? rows.length : total };
}

/**
 * Analytics computed from the audit log itself, for filters the analytics
 * endpoint does not support. Reads at most `cap` rows.
 */
export async function analyticsFromLogs(
	hours: number,
	filters: LogFilters,
	exclude: string[],
	cap = 5000
): Promise<Analytics & { capped: boolean }> {
	const end = new Date();
	const start = new Date(end.getTime() - hours * 3_600_000);
	const rows: LogEntry[] = [];
	const PAGE = 1000;
	let capped = false;
	for (let offset = 0; offset < cap; offset += PAGE) {
		const page = await fetchLogs(start, end, filters, PAGE, offset);
		rows.push(...page.entries);
		if (page.entries.length < PAGE) break;
		capped = offset + PAGE >= cap;
	}
	const kept = exclude.length ? rows.filter((r) => !exclude.includes(r.resource)) : rows;
	const byDay = hours > 48;
	const countries = new Map<string, { code: string; count: number; blocked: number }>();
	const buckets = new Map<string, { label: string; total: number; blocked: number }>();
	let blocked = 0;
	for (const row of kept) {
		if (!row.allowed) blocked++;
		if (row.location) {
			const c = countries.get(row.location) ?? { code: row.location, count: 0, blocked: 0 };
			c.count++;
			if (!row.allowed) c.blocked++;
			countries.set(row.location, c);
		}
		if (row.time) {
			const iso = row.time.toISOString();
			const label = byDay ? iso.slice(0, 10) : `${iso.slice(0, 13)}:00`;
			const b = buckets.get(label) ?? { label, total: 0, blocked: 0 };
			b.total++;
			if (!row.allowed) b.blocked++;
			buckets.set(label, b);
		}
	}
	const total = kept.length;
	return {
		total,
		allowed: total - blocked,
		blocked,
		blockRate: total > 0 ? (blocked / total) * 100 : 0,
		countries: [...countries.values()].sort((a, b) => b.count - a.count),
		timeline: [...buckets.values()].sort((a, b) => a.label.localeCompare(b.label)),
		capped
	};
}

const regionNames = new Intl.DisplayNames(['en'], { type: 'region' });

export function countryName(code: string): string {
	try {
		return regionNames.of(code.toUpperCase()) ?? code;
	} catch {
		return code;
	}
}
