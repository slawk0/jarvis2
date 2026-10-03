/**
 * Typed access to per-profile documents kept by the backend store.
 * Every document type lives here so there is one source of truth for the
 * shapes; deleting a profile removes all of them on the backend.
 */
import { api, type DataKey } from '$lib/ipc';
import { app } from './app.svelte';

export interface Runbook {
	id: string;
	name: string;
	command: string;
	sudo: boolean;
}

export interface SavedCommand {
	id: string;
	name: string;
	command: string;
}

export interface AlertThresholds {
	enabled: boolean;
	cpu: number;
	ram: number;
	disk: number;
}

/** Where a tool runs: on the host or inside a Docker container. */
export interface ExecTargetProfile {
	id: string;
	name: string;
	kind: 'host' | 'container';
	container: string;
}

export interface NginxTarget extends ExecTargetProfile {
	configRoot: string;
}

export interface LogAnalysisProfile extends ExecTargetProfile {
	server: 'nginx' | 'apache' | 'httpd' | 'traefik';
	/** Access-log path, or empty to read `docker logs` of the container. */
	logPath: string;
}

export interface LogSource {
	id: string;
	name: string;
	path: string;
}

export interface CrowdsecConfig {
	mode: 'auto' | 'native' | 'docker' | 'custom';
	container: string;
	/** Custom command prefix, e.g. `podman exec crowdsec cscli`. */
	prefix: string;
}

export interface WorkspaceLayoutDoc {
	version: 1;
	layout: unknown;
	focused: string | null;
	/** Pane id → tab id. */
	tabs: Record<string, string>;
}

interface Docs {
	runbooks: Runbook[];
	savedCommands: SavedCommand[];
	sftpBookmarks: string[];
	alertThresholds: AlertThresholds;
	nginxTargets: NginxTarget[];
	logAnalysisProfiles: LogAnalysisProfile[];
	logSources: LogSource[];
	crowdsecConfig: CrowdsecConfig;
	workspaceLayout: WorkspaceLayoutDoc;
}

/** Keys whose documents the backend owns and exposes through typed commands. */
type UiKey = Extract<DataKey, keyof Docs>;

/** Load a document of the active profile; `fallback` when nothing is stored. */
export async function loadDoc<K extends UiKey>(key: K, fallback: Docs[K]): Promise<Docs[K]> {
	const json = await api.profileDataGet(app.requireProfileId(), key);
	const value = JSON.parse(json) as Docs[K] | null;
	return value ?? fallback;
}

export async function saveDoc<K extends UiKey>(key: K, value: Docs[K]): Promise<void> {
	await api.profileDataSet(app.requireProfileId(), key, JSON.stringify(value));
}
