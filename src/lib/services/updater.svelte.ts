/** Auto-update flow: silent check after start, manual check, download, install, relaunch. */
import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { toast } from './toast.svelte';

export type UpdatePhase = 'idle' | 'checking' | 'available' | 'downloading' | 'installing' | 'error';

class UpdaterService {
	phase = $state<UpdatePhase>('idle');
	/** The update dialog is visible. */
	open = $state(false);
	version = $state('');
	notes = $state('');
	downloaded = $state(0);
	total = $state<number | null>(null);
	error = $state<string | null>(null);
	#update: Update | null = null;

	/** Dismissing is not allowed while the update is being applied. */
	get dismissible(): boolean {
		return this.phase !== 'downloading' && this.phase !== 'installing';
	}

	get progress(): number | null {
		return this.total ? Math.min(100, (this.downloaded / this.total) * 100) : null;
	}

	/**
	 * Look for an update. A silent check stays invisible unless one is found;
	 * a manual check reports "up to date" and errors too.
	 */
	async check(silent: boolean): Promise<void> {
		if (this.phase === 'checking' || !this.dismissible) return;
		this.phase = 'checking';
		this.error = null;
		try {
			const update = await check();
			if (!update) {
				this.phase = 'idle';
				if (!silent) toast.success('Jarvis is up to date');
				return;
			}
			this.#update = update;
			this.version = update.version;
			this.notes = update.body ?? '';
			this.phase = 'available';
			this.open = true;
		} catch (error) {
			this.phase = 'idle';
			if (!silent) toast.error(String(error), 'Could not check for updates');
		}
	}

	async install(): Promise<void> {
		const update = this.#update;
		if (!update) return;
		this.phase = 'downloading';
		this.downloaded = 0;
		this.total = null;
		this.error = null;
		try {
			await update.downloadAndInstall((event) => {
				if (event.event === 'Started') this.total = event.data.contentLength ?? null;
				else if (event.event === 'Progress') this.downloaded += event.data.chunkLength;
				else if (event.event === 'Finished') this.phase = 'installing';
			});
			await relaunch();
		} catch (error) {
			this.error = String(error);
			this.phase = 'error';
		}
	}

	later(): void {
		if (this.dismissible) this.open = false;
	}
}

export const updater = new UpdaterService();
