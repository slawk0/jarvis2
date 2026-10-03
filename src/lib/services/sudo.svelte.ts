/**
 * The single sudo prompt. `request()` opens the global dialog and resolves
 * `true` once the backend accepted a password, `false` if the user cancelled.
 * Concurrent requests share one dialog.
 */
import { commands } from '$lib/ipc/bindings';
import { toIpcError } from '$lib/ipc/errors';

interface SudoRequest {
	/** What needs elevation, shown in the dialog. */
	action?: string;
	expired?: boolean;
}

class SudoService {
	open = $state(false);
	action = $state('');
	expired = $state(false);
	busy = $state(false);
	error = $state<string | null>(null);
	/** Seconds until the lockout ends; 0 when not locked. */
	lockedFor = $state(0);

	#waiters: ((granted: boolean) => void)[] = [];
	#reason: string | null = null;
	#timer: ReturnType<typeof setInterval> | undefined;

	/**
	 * Give the next prompt triggered inside `fn` a meaningful description,
	 * e.g. `sudo.describe('Restart nginx', () => api.serviceAction(...))`.
	 */
	async describe<T>(reason: string, fn: () => Promise<T>): Promise<T> {
		const previous = this.#reason;
		this.#reason = reason;
		try {
			return await fn();
		} finally {
			this.#reason = previous;
		}
	}

	request(req: SudoRequest = {}): Promise<boolean> {
		return new Promise((resolve) => {
			this.#waiters.push(resolve);
			if (this.open) return;
			this.action = this.#reason ?? req.action ?? '';
			this.expired = req.expired ?? false;
			this.error = null;
			this.open = true;
		});
	}

	async submit(password: string): Promise<void> {
		if (this.busy || this.lockedFor > 0 || !password) return;
		this.busy = true;
		this.error = null;
		try {
			await commands.sudoAuthenticate(password);
			this.#finish(true);
		} catch (raw) {
			const error = toIpcError(raw);
			if (error.code === 'SUDO_PASSWORD_WRONG') {
				const left = Number(error.details);
				this.error =
					left > 0
						? `Incorrect password. ${left} attempt${left === 1 ? '' : 's'} left.`
						: 'Incorrect password.';
			} else if (error.code === 'SUDO_LOCKED') {
				this.#lock(Number(error.details) || 60);
			} else {
				this.error = error.message;
			}
		} finally {
			this.busy = false;
		}
	}

	cancel(): void {
		this.#finish(false);
	}

	#lock(seconds: number): void {
		this.lockedFor = seconds;
		clearInterval(this.#timer);
		this.#timer = setInterval(() => {
			this.lockedFor = Math.max(0, this.lockedFor - 1);
			if (this.lockedFor === 0) clearInterval(this.#timer);
		}, 1000);
	}

	#finish(granted: boolean): void {
		this.open = false;
		this.error = null;
		const waiters = this.#waiters;
		this.#waiters = [];
		for (const resolve of waiters) resolve(granted);
	}

	/** Drop pending prompts, e.g. when the connection goes away. */
	reset(): void {
		if (this.open) this.#finish(false);
	}
}

export const sudo = new SudoService();
