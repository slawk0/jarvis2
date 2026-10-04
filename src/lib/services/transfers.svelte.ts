/** Client side of the transfer engine: job list, conflict prompt, panel state. */
import {
	commands,
	events,
	type ConflictPolicy,
	type TransferJob,
	type TransferKind,
	type TransferRequest
} from '$lib/ipc/bindings';
import { api } from '$lib/ipc';
import { sudo } from './sudo.svelte';
import { toast } from './toast.svelte';

export interface TransferItem {
	kind: TransferKind;
	local?: string;
	remote: string;
	destination?: string;
	cleanupRemote?: boolean;
}

interface ConflictPrompt {
	paths: string[];
	resolve: (policy: ConflictPolicy | null) => void;
}

type BatchListener = (batchId: string, failed: number) => void;

class TransferService {
	jobs = $state<TransferJob[]>([]);
	panelOpen = $state(false);
	conflict = $state<ConflictPrompt | null>(null);
	#listeners = new Set<BatchListener>();
	#started = false;

	async init(): Promise<void> {
		if (this.#started) return;
		this.#started = true;
		await events.transferUpdate.listen(({ payload }) => {
			const index = this.jobs.findIndex((j) => j.id === payload.job.id);
			if (index >= 0) this.jobs[index] = payload.job;
			else {
				this.jobs.push(payload.job);
				this.panelOpen = true;
			}
			const code = payload.job.error?.code;
			if (
				payload.job.status === 'failed' &&
				(code === 'SUDO_PASSWORD_REQUIRED' || code === 'SUDO_PASSWORD_EXPIRED')
			) {
				void this.#elevate(payload.job.id);
			}
		});
		await events.transferBatchDone.listen(({ payload }) => {
			for (const listener of this.#listeners) listener(payload.batchId, payload.failed);
		});
	}

	#waitingForSudo = new Set<string>();

	/** A job needs root: ask once, then retry every job that was waiting for it. */
	async #elevate(jobId: string): Promise<void> {
		const first = this.#waitingForSudo.size === 0;
		this.#waitingForSudo.add(jobId);
		if (!first) return;
		const granted = await sudo.request({ action: 'transfer files that need root' });
		const ids = [...this.#waitingForSudo];
		this.#waitingForSudo.clear();
		if (granted) await commands.transferRetryFailed(ids).catch((e) => toast.error(e));
	}

	/** Called on connect: drop jobs of the previous session. */
	async sync(): Promise<void> {
		this.jobs = await commands.transferList().catch(() => []);
	}

	get active(): TransferJob[] {
		return this.jobs.filter((j) => j.status === 'queued' || j.status === 'running');
	}

	get totals() {
		const done = this.jobs.filter((j) => j.status === 'done' || j.status === 'skipped').length;
		const errors = this.jobs.filter((j) => j.status === 'failed').length;
		const speed = this.jobs.filter((j) => j.status === 'running').reduce((sum, j) => sum + j.speed, 0);
		return { done, errors, total: this.jobs.length, speed };
	}

	/** Get notified when a batch finishes (to refresh a file listing). */
	onBatchDone(listener: BatchListener): () => void {
		this.#listeners.add(listener);
		return () => this.#listeners.delete(listener);
	}

	/**
	 * Queue transfers. If targets already exist the user chooses overwrite,
	 * skip or rename for the whole batch. Resolves the batch id, or null when
	 * cancelled.
	 */
	async enqueue(items: TransferItem[]): Promise<string | null> {
		if (items.length === 0) return null;
		const build = (policy: ConflictPolicy): TransferRequest[] =>
			items.map((item) => ({
				kind: item.kind,
				local: item.local ?? '',
				remote: item.remote,
				destination: item.destination ?? '',
				conflict: policy,
				cleanupRemote: item.cleanupRemote ?? false
			}));
		try {
			let policy: ConflictPolicy = 'overwrite';
			const existing = await api.transferConflicts(build('overwrite'));
			if (existing.length > 0) {
				const choice = await new Promise<ConflictPolicy | null>((resolve) => {
					this.conflict = { paths: existing, resolve };
				});
				this.conflict = null;
				if (choice === null) return null;
				policy = choice;
			}
			return await api.transferEnqueue(build(policy));
		} catch (error) {
			toast.error(error, 'Could not start the transfer');
			return null;
		}
	}

	cancel(jobId?: string, batchId?: string): void {
		void commands.transferCancel(jobId ?? null, batchId ?? null);
	}

	async retryFailed(): Promise<void> {
		try {
			await api.transferRetryFailed(null);
		} catch (error) {
			toast.error(error);
		}
	}

	async clearCompleted(): Promise<void> {
		this.jobs = await commands.transferClearCompleted();
	}
}

export const transfers = new TransferService();
