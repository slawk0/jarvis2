/**
 * The shared pattern for "data loaded from the server": loading / error /
 * data, explicit refresh, and quiet background polling that pauses while the
 * tab is hidden or the server is offline.
 */
import { api, apiQuiet, IpcError, toIpcError } from '$lib/ipc';
import { app } from '$lib/services/app.svelte';

type Io = typeof api;

export class Resource<T> {
	data = $state<T | undefined>(undefined);
	error = $state<IpcError | null>(null);
	loading = $state(false);
	#loader: (io: Io) => Promise<T>;
	#seq = 0;

	constructor(loader: (io: Io) => Promise<T>) {
		this.#loader = loader;
	}

	get loaded(): boolean {
		return this.data !== undefined;
	}

	/**
	 * Load the data. A quiet load never opens the sudo dialog and keeps stale
	 * data on failure — use it for polling.
	 */
	async load(quiet = false): Promise<void> {
		const seq = ++this.#seq;
		if (!quiet) this.loading = true;
		try {
			const data = await this.#loader(quiet ? apiQuiet : api);
			if (seq !== this.#seq) return;
			this.data = data;
			this.error = null;
		} catch (raw) {
			if (seq !== this.#seq) return;
			if (quiet && this.data !== undefined) return;
			this.error = toIpcError(raw);
		} finally {
			if (seq === this.#seq) this.loading = false;
		}
	}

	refresh = (): Promise<void> => this.load(false);

	/** Replace the data locally (optimistic updates). */
	set(data: T): void {
		this.#seq++;
		this.data = data;
		this.error = null;
		this.loading = false;
	}

	reset(): void {
		this.#seq++;
		this.data = undefined;
		this.error = null;
		this.loading = false;
	}
}

export function resource<T>(loader: (io: Io) => Promise<T>): Resource<T> {
	return new Resource(loader);
}

/**
 * Run `fn` every `interval()` ms while `active()` is true and the server is
 * online. Runs never overlap. Call during component initialisation.
 */
export function poll(
	active: () => boolean,
	interval: number | (() => number),
	fn: () => unknown,
	options: { immediate?: boolean } = {}
): void {
	$effect(() => {
		if (!active() || !app.online) return;
		const ms = typeof interval === 'function' ? interval() : interval;
		let stopped = false;
		let timer: ReturnType<typeof setTimeout> | undefined;
		const tick = async () => {
			try {
				await fn();
			} catch {
				// Polling failures are reported by whatever `fn` loads into.
			}
			if (!stopped) timer = setTimeout(tick, ms);
		};
		if (options.immediate) void tick();
		else timer = setTimeout(tick, ms);
		return () => {
			stopped = true;
			clearTimeout(timer);
		};
	});
}

/**
 * Load a resource when its tab first becomes visible, and keep it fresh with
 * quiet polling while visible. The usual one-liner for a tab's main data.
 */
export function autoLoad<T>(
	res: Resource<T>,
	visible: () => boolean,
	pollMs?: number | (() => number),
	polling: () => boolean = () => true
): void {
	let started = false;
	$effect(() => {
		if (visible() && app.online && !started) {
			started = true;
			void res.refresh();
		}
	});
	if (pollMs !== undefined) {
		poll(
			() => visible() && polling(),
			pollMs,
			() => res.load(true)
		);
	}
}

/** Run an action with a busy flag and error toast; resolves true on success. */
export async function attempt(
	action: () => Promise<unknown>,
	onError: (error: IpcError) => void
): Promise<boolean> {
	try {
		await action();
		return true;
	} catch (raw) {
		const error = toIpcError(raw);
		if (error.code !== 'CANCELLED') onError(error);
		return false;
	}
}
