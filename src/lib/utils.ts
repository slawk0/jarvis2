import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]): string {
	return twMerge(clsx(inputs));
}

/* Prop helpers used by the shadcn-svelte primitives. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type WithoutChild<T> = T extends { child?: any } ? Omit<T, 'child'> : T;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type WithoutChildren<T> = T extends { children?: any } ? Omit<T, 'children'> : T;
export type WithoutChildrenOrChild<T> = WithoutChildren<WithoutChild<T>>;
export type WithElementRef<T, U extends HTMLElement = HTMLElement> = T & { ref?: U | null };

export function uid(): string {
	return crypto.randomUUID();
}

export function clamp(value: number, min: number, max: number): number {
	return Math.min(max, Math.max(min, value));
}

/** Debounce a function; the returned function also exposes `cancel`. */
export function debounce<A extends unknown[]>(
	fn: (...args: A) => void,
	ms: number
): ((...args: A) => void) & { cancel: () => void } {
	let timer: ReturnType<typeof setTimeout> | undefined;
	const wrapped = (...args: A) => {
		clearTimeout(timer);
		timer = setTimeout(() => fn(...args), ms);
	};
	wrapped.cancel = () => clearTimeout(timer);
	return wrapped;
}

export function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Case-insensitive "does any of these fields contain the query". */
export function matches(query: string, ...fields: (string | number | null | undefined)[]): boolean {
	const q = query.trim().toLowerCase();
	if (!q) return true;
	return fields.some((f) => f != null && String(f).toLowerCase().includes(q));
}

export type SortDir = 'asc' | 'desc';

export function compare(a: unknown, b: unknown): number {
	if (a == null && b == null) return 0;
	if (a == null) return 1;
	if (b == null) return -1;
	if (typeof a === 'number' && typeof b === 'number') return a - b;
	if (typeof a === 'boolean' && typeof b === 'boolean') return Number(a) - Number(b);
	return String(a).localeCompare(String(b), undefined, { numeric: true, sensitivity: 'base' });
}

export async function copyText(text: string): Promise<void> {
	await navigator.clipboard.writeText(text);
}
