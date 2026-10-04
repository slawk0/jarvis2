/**
 * The tab registry. Sidebar, pane tab picker and pane rendering are all
 * driven from this one list.
 */
import type { Component } from 'svelte';
import type { Profile, Tool } from '$lib/ipc';
import Activity from '@lucide/svelte/icons/activity';
import Archive from '@lucide/svelte/icons/archive';
import Boxes from '@lucide/svelte/icons/boxes';
import CalendarClock from '@lucide/svelte/icons/calendar-clock';
import ChartColumn from '@lucide/svelte/icons/chart-column';
import Cog from '@lucide/svelte/icons/cog';
import Cpu from '@lucide/svelte/icons/cpu';
import Database from '@lucide/svelte/icons/database';
import DatabaseBackup from '@lucide/svelte/icons/database-backup';
import FolderTree from '@lucide/svelte/icons/folder-tree';
import Globe from '@lucide/svelte/icons/globe';
import HardDrive from '@lucide/svelte/icons/hard-drive';
import LayoutDashboard from '@lucide/svelte/icons/layout-dashboard';
import ListChecks from '@lucide/svelte/icons/list-checks';
import Network from '@lucide/svelte/icons/network';
import Radar from '@lucide/svelte/icons/radar';
import ScrollText from '@lucide/svelte/icons/scroll-text';
import Server from '@lucide/svelte/icons/server';
import Shield from '@lucide/svelte/icons/shield';
import ShieldAlert from '@lucide/svelte/icons/shield-alert';
import SquareTerminal from '@lucide/svelte/icons/square-terminal';
import Timer from '@lucide/svelte/icons/timer';
import Users from '@lucide/svelte/icons/users';
import Variable from '@lucide/svelte/icons/variable';
import Waypoints from '@lucide/svelte/icons/waypoints';
import Wrench from '@lucide/svelte/icons/wrench';

/** Props every tab component receives. */
export interface TabProps {
	/** False while the tab is kept alive in the background: pause polling. */
	visible: boolean;
	profile: Profile;
}

/** What a tab component may export. */
export interface TabExports {
	refresh?: () => void | Promise<void>;
	/** The already-active tab was clicked again: return to the root view. */
	onReselect?: () => void;
}

export type TabComponent = Component<TabProps, TabExports>;

export type TabId =
	| 'dashboard'
	| 'terminal'
	| 'runbooks'
	| 'services'
	| 'docker'
	| 'processes'
	| 'timers'
	| 'cron'
	| 'disks'
	| 'maintenance'
	| 'users'
	| 'env'
	| 'nginx'
	| 'pangolin'
	| 'network'
	| 'netdiag'
	| 'firewall'
	| 'crowdsec'
	| 'files'
	| 'databases'
	| 'backups'
	| 'restic'
	| 'logs'
	| 'loganalysis';

export interface TabDef {
	id: TabId;
	label: string;
	icon: Component<{ class?: string }>;
	category: string;
	/** Server-side tools the whole tab needs; it is wrapped in a dependency guard. */
	requires: Tool[];
	load: () => Promise<{ default: TabComponent }>;
}

export const CATEGORIES = [
	'Overview',
	'System',
	'Network & Web',
	'Security',
	'Data & Storage',
	'Monitoring'
] as const;

const tab = (
	id: TabId,
	label: string,
	icon: TabDef['icon'],
	category: (typeof CATEGORIES)[number],
	load: TabDef['load'],
	requires: Tool[] = []
): TabDef => ({ id, label, icon, category, load, requires });

export const TABS: TabDef[] = [
	tab(
		'dashboard',
		'Dashboard',
		LayoutDashboard,
		'Overview',
		() => import('$lib/features/dashboard/Dashboard.svelte')
	),
	tab(
		'terminal',
		'Terminal',
		SquareTerminal,
		'Overview',
		() => import('$lib/features/terminal/Terminal.svelte')
	),
	tab('runbooks', 'Runbooks', ListChecks, 'Overview', () => import('$lib/features/runbooks/Runbooks.svelte')),

	tab('services', 'Services', Cog, 'System', () => import('$lib/features/services/Services.svelte'), [
		'systemd'
	]),
	tab('docker', 'Docker', Boxes, 'System', () => import('$lib/features/docker/Docker.svelte'), ['docker']),
	tab('processes', 'Processes', Cpu, 'System', () => import('$lib/features/processes/Processes.svelte')),
	tab('timers', 'Systemd Timers', Timer, 'System', () => import('$lib/features/timers/Timers.svelte'), [
		'systemd'
	]),
	tab('cron', 'Cron', CalendarClock, 'System', () => import('$lib/features/cron/Cron.svelte'), ['cron']),
	tab('disks', 'Disks', HardDrive, 'System', () => import('$lib/features/disks/Disks.svelte')),
	tab(
		'maintenance',
		'Maintenance',
		Wrench,
		'System',
		() => import('$lib/features/maintenance/Maintenance.svelte')
	),
	tab('users', 'Users', Users, 'System', () => import('$lib/features/users/Users.svelte')),
	tab('env', 'Env Variables', Variable, 'System', () => import('$lib/features/env/Env.svelte')),

	tab('nginx', 'Nginx Manager', Server, 'Network & Web', () => import('$lib/features/nginx/Nginx.svelte')),
	tab(
		'pangolin',
		'Pangolin Proxy',
		Waypoints,
		'Network & Web',
		() => import('$lib/features/pangolin/Pangolin.svelte')
	),
	tab(
		'network',
		'Network / Ports',
		Network,
		'Network & Web',
		() => import('$lib/features/network/Network.svelte')
	),
	tab(
		'netdiag',
		'Net Diagnostics',
		Radar,
		'Network & Web',
		() => import('$lib/features/netdiag/NetDiag.svelte')
	),

	tab('firewall', 'Firewall', Shield, 'Security', () => import('$lib/features/firewall/Firewall.svelte')),
	tab(
		'crowdsec',
		'CrowdSec',
		ShieldAlert,
		'Security',
		() => import('$lib/features/crowdsec/CrowdSec.svelte')
	),

	tab(
		'files',
		'Files (SFTP)',
		FolderTree,
		'Data & Storage',
		() => import('$lib/features/files/Files.svelte')
	),
	tab(
		'databases',
		'Databases',
		Database,
		'Data & Storage',
		() => import('$lib/features/databases/Databases.svelte')
	),
	tab('backups', 'Backups', Archive, 'Data & Storage', () => import('$lib/features/backups/Backups.svelte')),
	tab(
		'restic',
		'Restic Backups',
		DatabaseBackup,
		'Data & Storage',
		() => import('$lib/features/restic/Restic.svelte'),
		['restic']
	),

	tab('logs', 'Logs', ScrollText, 'Monitoring', () => import('$lib/features/logs/Logs.svelte')),
	tab(
		'loganalysis',
		'Log Analysis',
		ChartColumn,
		'Monitoring',
		() => import('$lib/features/loganalysis/LogAnalysis.svelte')
	)
];

const BY_ID = new Map(TABS.map((t) => [t.id, t]));

export function tabDef(id: string): TabDef | undefined {
	return BY_ID.get(id as TabId);
}

export function isTabId(id: unknown): id is TabId {
	return typeof id === 'string' && BY_ID.has(id as TabId);
}

export const TAB_COLORS = ['red', 'orange', 'yellow', 'green', 'teal', 'blue', 'purple', 'pink'] as const;
export type TabColor = (typeof TAB_COLORS)[number];

export const DEFAULT_TAB: TabId = 'dashboard';

// Icons re-exported for places that need a generic "activity" glyph.
export { Activity, Globe };
