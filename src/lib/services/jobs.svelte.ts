/**
 * Client side of the backend job mechanism.
 *
 * One global listener receives every job event from app start, so output is
 * never missed between "command returned a jobId" and "a view subscribed".
 * The Running Jobs panel and any inline output view read the same `Job`.
 */
import { commands, events, type JobStream } from '$lib/ipc/bindings';
import { IpcError, toIpcError } from '$lib/ipc/errors';

export type JobStatus = 'running' | 'done' | 'failed' | 'cancelled';

const MAX_LOG = 2 * 1024 * 1024;
const MAX_VISIBLE_JOBS = 100;

export interface JobResult {
	status: JobStatus;
	exitCode: number | null;
	error: IpcError | null;
	log: string;
}

type OutputListener = (chunk: string, stream: JobStream) => void;

export class Job {
	readonly id: string;
	title = $state('');
	detail = $state('');
	visible = $state(false);
	status = $state<JobStatus>('running');
	startedAt = $state(Date.now());
	endedAt = $state<number | null>(null);
	exitCode = $state<number | null>(null);
	error = $state<IpcError | null>(null);
	/** Bumped (at most once per frame) whenever `log` grew. */
	revision = $state(0);

	/** Full output so far. Not reactive itself; read it after `revision`. */
	log = '';
	#listeners = new Set<OutputListener>();
	#waiters: ((result: JobResult) => void)[] = [];
	#frame = 0;

	constructor(id: string) {
		this.id = id;
	}

	get running(): boolean {
		return this.status === 'running';
	}

	get result(): JobResult {
		return { status: this.status, exitCode: this.exitCode, error: this.error, log: this.log };
	}

	append(chunk: string, stream: JobStream): void {
		this.log += chunk;
		if (this.log.length > MAX_LOG) this.log = this.log.slice(-MAX_LOG / 2);
		for (const listener of this.#listeners) listener(chunk, stream);
		if (!this.#frame) {
			this.#frame = requestAnimationFrame(() => {
				this.#frame = 0;
				this.revision++;
			});
		}
	}

	finish(exitCode: number | null, error: IpcError | null, cancelled: boolean): void {
		this.exitCode = exitCode;
		this.error = error;
		this.endedAt = Date.now();
		this.status = cancelled ? 'cancelled' : error || exitCode !== 0 ? 'failed' : 'done';
		this.revision++;
		const waiters = this.#waiters;
		this.#waiters = [];
		for (const resolve of waiters) resolve(this.result);
	}

	/** Receive output chunks as they arrive. Returns an unsubscribe function. */
	onOutput(listener: OutputListener): () => void {
		this.#listeners.add(listener);
		return () => this.#listeners.delete(listener);
	}

	/** Resolves when the job has ended (immediately if it already has). */
	wait(): Promise<JobResult> {
		if (!this.running) return Promise.resolve(this.result);
		return new Promise((resolve) => this.#waiters.push(resolve));
	}

	stop(): Promise<void> {
		return commands.jobCancel(this.id);
	}
}

class JobsService {
	all = $state<Job[]>([]);
	panelOpen = $state(false);
	/** Job shown in the panel's detail view. */
	selectedId = $state<string | null>(null);
	#started = false;

	/** Jobs that belong in the Running Jobs panel, newest first. */
	get visible(): Job[] {
		return this.all.filter((j) => j.visible).reverse();
	}

	get runningCount(): number {
		return this.all.filter((j) => j.visible && j.running).length;
	}

	async init(): Promise<void> {
		if (this.#started) return;
		this.#started = true;
		await Promise.all([
			events.jobStarted.listen(({ payload }) => {
				const job = this.get(payload.jobId);
				job.title = payload.title;
				job.detail = payload.detail;
				job.visible = payload.visible;
				job.startedAt = payload.startedAt;
				this.#trim();
			}),
			events.jobOutput.listen(({ payload }) => {
				this.get(payload.jobId).append(payload.chunk, payload.stream);
			}),
			events.jobDone.listen(({ payload }) => {
				const job = this.get(payload.jobId);
				const error = payload.error && !payload.cancelled ? toIpcError(payload.error) : null;
				job.finish(payload.exitCode, error, payload.cancelled);
				// Streams nobody lists (log follows, live stats) are dropped once done.
				if (!job.visible) setTimeout(() => this.#remove(job.id), 60_000);
			})
		]);
	}

	/** The job with this id; created on first reference so events can't race. */
	get(id: string): Job {
		let job = this.all.find((j) => j.id === id);
		if (!job) {
			job = new Job(id);
			this.all.push(job);
			// Return the reactive proxy stored in the array.
			job = this.all[this.all.length - 1];
		}
		return job;
	}

	/**
	 * Start a job through `start` (an `api.*` call returning a job id) and
	 * resolve when it ends. Throws if it fails, unless `allowFailure` is set.
	 */
	async run(
		start: () => Promise<string>,
		options: { onStart?: (job: Job) => void; allowFailure?: boolean } = {}
	): Promise<JobResult> {
		const job = this.get(await start());
		options.onStart?.(job);
		const result = await job.wait();
		if (options.allowFailure || result.status === 'done') return result;
		if (result.status === 'cancelled') throw new IpcError('CANCELLED');
		throw result.error ?? new IpcError('COMMAND_FAILED', lastLines(result.log, 6));
	}

	show(id?: string): void {
		this.panelOpen = true;
		this.selectedId = id ?? null;
	}

	clearFinished(): void {
		this.all = this.all.filter((j) => j.running || !j.visible);
		if (this.selectedId && !this.all.some((j) => j.id === this.selectedId)) {
			this.selectedId = null;
		}
	}

	#remove(id: string): void {
		this.all = this.all.filter((j) => j.id !== id);
	}

	#trim(): void {
		const finished = this.all.filter((j) => j.visible && !j.running);
		const excess = this.all.filter((j) => j.visible).length - MAX_VISIBLE_JOBS;
		if (excess > 0) {
			const drop = new Set(finished.slice(0, excess).map((j) => j.id));
			this.all = this.all.filter((j) => !drop.has(j.id));
		}
	}
}

function lastLines(text: string, count: number): string {
	return text.trimEnd().split('\n').slice(-count).join('\n');
}

export const jobs = new JobsService();
