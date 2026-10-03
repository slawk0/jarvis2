/** The one toast service. Errors accept anything thrown. */
import { toIpcError } from '$lib/ipc/errors';
import { uid } from '$lib/utils';

export type ToastKind = 'success' | 'info' | 'warning' | 'error';

export interface Toast {
	id: string;
	kind: ToastKind;
	message: string;
	/** Longer text shown under the message (e.g. command output). */
	detail?: string;
}

const DURATION: Record<ToastKind, number> = {
	success: 4000,
	info: 4000,
	warning: 5000,
	error: 8000
};

class ToastService {
	items = $state<Toast[]>([]);
	#timers = new Map<string, ReturnType<typeof setTimeout>>();

	#push(kind: ToastKind, message: string, detail?: string): string {
		// Collapse identical toasts (polling failures would otherwise stack up).
		const duplicate = this.items.find((t) => t.kind === kind && t.message === message);
		if (duplicate) {
			this.#arm(duplicate.id, kind);
			return duplicate.id;
		}
		const id = uid();
		this.items.push({ id, kind, message, detail });
		if (this.items.length > 5) this.dismiss(this.items[0].id);
		this.#arm(id, kind);
		return id;
	}

	#arm(id: string, kind: ToastKind): void {
		clearTimeout(this.#timers.get(id));
		this.#timers.set(
			id,
			setTimeout(() => this.dismiss(id), DURATION[kind])
		);
	}

	/** Keep a toast open while the pointer is over it. */
	hold(id: string): void {
		clearTimeout(this.#timers.get(id));
	}

	release(id: string): void {
		const toast = this.items.find((t) => t.id === id);
		if (toast) this.#arm(id, toast.kind);
	}

	success(message: string): void {
		this.#push('success', message);
	}

	info(message: string): void {
		this.#push('info', message);
	}

	warning(message: string, detail?: string): void {
		this.#push('warning', message, detail);
	}

	/** Show an error. A cancelled action (e.g. dismissed sudo prompt) is silent. */
	error(error: unknown, context?: string): void {
		if (typeof error === 'string') {
			this.#push('error', context ? `${context}: ${error}` : error);
			return;
		}
		const e = toIpcError(error);
		if (e.code === 'CANCELLED') return;
		const title = context ?? e.title;
		const detail = context ? e.message : (e.details ?? undefined);
		const hidden = e.is('SUDO_PASSWORD_WRONG', 'SUDO_LOCKED', 'HOST_KEY_UNKNOWN', 'HOST_KEY_CHANGED');
		this.#push('error', title, hidden ? undefined : (detail ?? undefined));
	}

	dismiss(id: string): void {
		clearTimeout(this.#timers.get(id));
		this.#timers.delete(id);
		this.items = this.items.filter((t) => t.id !== id);
	}
}

export const toast = new ToastService();
