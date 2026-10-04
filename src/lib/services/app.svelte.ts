/**
 * Connection state: saved profiles, the active session and its link status.
 * One server is active at a time; switching resets every feature (the
 * workspace is keyed on `sessionKey`).
 */
import {
	commands,
	events,
	type HostKeyIssue,
	type Profile,
	type ProfileInput,
	type ProfileView,
	type SessionInfo
} from '$lib/ipc/bindings';
import { IpcError, toIpcError } from '$lib/ipc/errors';
import { sudo } from './sudo.svelte';
import { toast } from './toast.svelte';
import { transfers } from './transfers.svelte';

export type LinkStatus = 'online' | 'offline' | 'reconnecting' | 'switching';

interface HostKeyPrompt {
	issue: HostKeyIssue;
	profileId: string;
}

class AppService {
	profiles = $state<ProfileView[]>([]);
	profilesLoaded = $state(false);
	session = $state<SessionInfo | null>(null);
	status = $state<LinkStatus>('online');
	reconnectAttempt = $state(0);
	/** Profile currently being connected to (spinner on its card). */
	connectingId = $state<string | null>(null);
	connectError = $state<IpcError | null>(null);
	hostKeyPrompt = $state<HostKeyPrompt | null>(null);
	/** Shown while the server reboots; reconnect attempts continue behind it. */
	rebooting = $state(false);
	/** Changes on every successful connect so the workspace remounts. */
	sessionKey = $state(0);

	get profile(): Profile | null {
		const id = this.session?.profileId;
		return this.profiles.find((p) => p.profile.id === id)?.profile ?? null;
	}

	get connected(): boolean {
		return this.session !== null;
	}

	get online(): boolean {
		return this.session !== null && this.status === 'online';
	}

	async init(): Promise<void> {
		await events.connectionStatus.listen(({ payload }) => {
			if (payload.profileId !== this.session?.profileId) return;
			this.status = payload.state;
			this.reconnectAttempt = payload.attempt;
			if (payload.state === 'online') {
				if (this.rebooting) toast.success('Server is back online');
				this.rebooting = false;
			} else {
				sudo.reset();
			}
		});
		await this.loadProfiles();
		// Recover the session after a webview reload; otherwise auto-connect the default.
		const live = await commands.sessionInfo();
		if (live) {
			this.session = live;
			this.sessionKey++;
			return;
		}
		const preferred = this.profiles.find((p) => p.profile.isDefault);
		if (preferred) void this.connect(preferred.profile.id);
	}

	async loadProfiles(): Promise<void> {
		this.profiles = await commands.profilesList();
		this.profilesLoaded = true;
	}

	/** Connect to a profile. Resolves `true` when a session is live. */
	async connect(profileId: string): Promise<boolean> {
		if (this.connectingId) return false;
		const switching = this.session !== null;
		this.connectingId = profileId;
		this.connectError = null;
		if (switching) this.status = 'switching';
		sudo.reset();
		try {
			const outcome = await commands.connect(profileId);
			if (outcome.status === 'hostKey') {
				this.session = null;
				this.hostKeyPrompt = { issue: outcome.issue, profileId };
				return false;
			}
			this.session = outcome.session;
			this.status = 'online';
			this.rebooting = false;
			this.sessionKey++;
			void transfers.sync();
			return true;
		} catch (raw) {
			// The previous session was torn down before connecting.
			this.session = null;
			this.connectError = toIpcError(raw);
			return false;
		} finally {
			this.connectingId = null;
		}
	}

	/** Answer the fingerprint prompt: trust (or replace) the key and connect. */
	async trustHostKey(): Promise<void> {
		const prompt = this.hostKeyPrompt;
		if (!prompt) return;
		const { issue, profileId } = prompt;
		this.hostKeyPrompt = null;
		try {
			await commands.knownHostTrust(issue.host, issue.port, issue.key, issue.knownFingerprints.length > 0);
			await this.connect(profileId);
		} catch (raw) {
			this.connectError = toIpcError(raw);
		}
	}

	rejectHostKey(): void {
		this.hostKeyPrompt = null;
	}

	async disconnect(): Promise<void> {
		sudo.reset();
		this.session = null;
		this.rebooting = false;
		this.status = 'online';
		await commands.disconnect().catch(() => {});
	}

	reconnectNow(): void {
		void commands.reconnectNow();
	}

	async saveProfile(input: ProfileInput): Promise<ProfileView> {
		const saved = await commands.profileSave(input);
		await this.loadProfiles();
		return saved;
	}

	async deleteProfile(profileId: string): Promise<void> {
		if (this.session?.profileId === profileId) this.session = null;
		await commands.profileDelete(profileId);
		await this.loadProfiles();
	}

	async setDefault(profileId: string | null): Promise<void> {
		await commands.profileSetDefault(profileId);
		await this.loadProfiles();
	}

	/** Id of the active profile; throws when disconnected. */
	requireProfileId(): string {
		const id = this.session?.profileId;
		if (!id) throw new IpcError('NOT_CONNECTED');
		return id;
	}
}

export const app = new AppService();
