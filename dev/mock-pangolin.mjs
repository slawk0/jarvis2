// A small in-memory stand-in for the Pangolin Integration API, for developing
// and testing the Pangolin tab without a real Pangolin instance.
//
//   node dev/mock-pangolin.mjs [port]        (default 3939)
//
// In Jarvis: Pangolin → Settings → API URL http://127.0.0.1:3939, API key "test.key".
// A key ending in ".scoped" behaves like an organisation-scoped key (cannot list orgs).

import { createServer } from 'node:http';

const PORT = Number(process.argv[2] ?? 3939);
const ORG = 'acme';
let nextId = 100;
const id = () => ++nextId;

const db = {
	sites: [
		{
			siteId: 1,
			niceId: 'brave-otter',
			name: 'Home lab',
			type: 'newt',
			online: true,
			subnet: '100.89.128.4/30',
			address: '100.90.128.1/24',
			pubKey: 'mF3kq0pWm2n1Yx8v0s7T5u6r4e3w2q1z9x8c7v6b5n4='
		},
		{
			site: true,
			siteId: 2,
			niceId: 'calm-heron',
			name: 'Office',
			type: 'wireguard',
			online: false,
			subnet: '100.89.128.8/30',
			address: '',
			pubKey: 'Zx9c8v7b6n5m4l3k2j1h0g9f8d7s6a5p4o3i2u1y0t='
		},
		{
			siteId: 3,
			niceId: 'local',
			name: 'This server',
			type: 'local',
			online: false,
			subnet: '',
			address: '',
			pubKey: ''
		}
	],
	siteResources: [
		{
			siteResourceId: 11,
			siteId: 1,
			siteName: 'Home lab',
			name: 'NAS',
			mode: 'host',
			destination: '192.168.1.20',
			alias: 'nas.internal',
			tcpPortRangeString: '*',
			udpPortRangeString: '',
			disableIcmp: false,
			userIds: ['u1'],
			roleIds: [2],
			clientIds: []
		},
		{
			siteResourceId: 12,
			siteId: 2,
			siteName: 'Office',
			name: 'Office LAN',
			mode: 'cidr',
			destination: '10.10.0.0/24',
			tcpPortRangeString: '22,80,443',
			udpPortRangeString: '53',
			disableIcmp: true,
			userIds: [],
			roleIds: [1],
			clientIds: []
		}
	],
	resources: [
		{
			resourceId: 21,
			niceId: 'grafana',
			name: 'Grafana',
			http: true,
			protocol: 'tcp',
			mode: 'http',
			ssl: true,
			fullDomain: 'grafana.example.com',
			subdomain: 'grafana',
			domainId: 'd1',
			stickySession: false,
			enabled: true,
			postAuthPath: ''
		},
		{
			resourceId: 22,
			niceId: 'game',
			name: 'Game server',
			http: false,
			protocol: 'udp',
			mode: 'udp',
			proxyPort: 27015,
			stickySession: false,
			enabled: true
		}
	],
	targets: [
		{ targetId: 31, resourceId: 21, siteId: 1, ip: '192.168.1.30', port: 3000, method: 'http', enabled: true }
	],
	domains: [
		{ domainId: 'd1', baseDomain: 'example.com', verified: true },
		{ domainId: 'd2', baseDomain: 'example.org', verified: true }
	],
	users: [
		{
			id: 'u1',
			email: 'ada@example.com',
			username: 'ada@example.com',
			name: 'Ada Lovelace',
			twoFactorEnabled: true,
			isOwner: true,
			roles: [{ roleId: 1, roleName: 'Admin' }]
		},
		{
			id: 'u2',
			email: 'linus@example.com',
			username: 'linus@example.com',
			name: 'Linus',
			twoFactorEnabled: false,
			isOwner: false,
			idpName: 'Authentik',
			roles: [{ roleId: 2, roleName: 'Member' }]
		}
	],
	roles: [
		{ roleId: 1, name: 'Admin', description: 'Full access', isAdmin: true },
		{
			roleId: 2,
			name: 'Member',
			description: 'Default role',
			isAdmin: false,
			requireDeviceApproval: false,
			allowSsh: true,
			sshSudoMode: 'commands',
			sshSudoCommands: ['/usr/bin/systemctl restart nginx'],
			sshCreateHomeDir: true,
			sshUnixGroups: ['docker']
		}
	],
	idps: [{ idpId: 1, name: 'Authentik', type: 'oidc', autoProvision: true }],
	invitations: [
		{
			inviteId: 'inv1',
			email: 'new@example.com',
			expiresAt: Date.now() + 86_400_000,
			roles: [{ roleId: 2, roleName: 'Member' }]
		}
	],
	devices: [
		{
			clientId: 41,
			name: "Ada's MacBook",
			userEmail: 'ada@example.com',
			subnet: '100.90.129.2/24',
			online: true,
			blocked: false,
			archived: false,
			agent: 'macos',
			olmVersion: '1.4.0',
			lastSeen: Math.floor(Date.now() / 1000) - 60
		},
		{
			clientId: 42,
			name: 'Pixel 9',
			userEmail: 'linus@example.com',
			subnet: '100.90.129.3/24',
			online: false,
			blocked: true,
			archived: false,
			agent: 'android',
			olmVersion: '1.3.2',
			lastSeen: Math.floor(Date.now() / 1000) - 86_400 * 3
		}
	],
	clients: [{ clientId: 51, name: 'ci-runner' }],
	accessTokens: [
		{
			accessTokenId: 'tok_8f3k2j',
			title: 'Share with contractor',
			resourceName: 'Grafana',
			resourceId: 21,
			expiresAt: Date.now() + 7 * 86_400_000
		}
	]
};

// A deterministic request log spread over 30 days.
const COUNTRIES = ['US', 'DE', 'PL', 'GB', 'FR', 'NL', 'CN', 'RU', 'BR', 'IN', 'JP', 'AU', 'CA', 'SE', 'UA'];
const PATHS = ['/', '/login', '/api/health', '/api/dashboards', '/wp-login.php', '/.env', '/static/app.js'];
const log = [];
let seed = 42;
const rand = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
for (let i = 0; i < 2600; i++) {
	const age = Math.floor(rand() * rand() * 30 * 86_400);
	const path = PATHS[Math.floor(rand() * PATHS.length)];
	const hostile = path === '/wp-login.php' || path === '/.env';
	const allowed = !hostile && rand() > 0.12;
	const country = COUNTRIES[Math.floor(rand() * rand() * COUNTRIES.length)];
	log.push({
		id: i + 1,
		timestamp: Math.floor(Date.now() / 1000) - age,
		action: allowed,
		reason: allowed ? (rand() > 0.5 ? 107 : 100) : hostile ? 202 : 203,
		ip: `${20 + Math.floor(rand() * 200)}.${Math.floor(rand() * 255)}.${Math.floor(rand() * 255)}.${1 + Math.floor(rand() * 254)}`,
		location: country,
		resourceId: 21,
		resourceName: 'Grafana',
		host: 'grafana.example.com',
		path,
		method: path.startsWith('/api') && rand() > 0.7 ? 'POST' : 'GET',
		actor: allowed && rand() > 0.5 ? 'ada@example.com' : '',
		actorType: 'user',
		tls: true,
		userAgent: 'Mozilla/5.0 (X11; Linux x86_64)',
		headers: JSON.stringify({ 'accept-language': 'en-GB', 'x-forwarded-proto': 'https' }),
		metadata: JSON.stringify({ tlsVersion: 'TLS 1.3', cipher: 'TLS_AES_128_GCM_SHA256' })
	});
}
log.sort((a, b) => b.timestamp - a.timestamp);

function filterLog(q) {
	const start = q.get('timeStart') ? Date.parse(q.get('timeStart')) / 1000 : 0;
	const end = q.get('timeEnd') ? Date.parse(q.get('timeEnd')) / 1000 : Infinity;
	return log.filter(
		(r) =>
			r.timestamp >= start &&
			r.timestamp <= end &&
			(!q.get('action') || String(r.action) === q.get('action')) &&
			(!q.get('method') || r.method === q.get('method')) &&
			(!q.get('location') || r.location === q.get('location')) &&
			(!q.get('host') || r.host.includes(q.get('host'))) &&
			(!q.get('path') || r.path.includes(q.get('path'))) &&
			(!q.get('reason') || String(r.reason) === q.get('reason')) &&
			(!q.get('actor') || r.actor.includes(q.get('actor'))) &&
			(!q.get('resourceId') || String(r.resourceId) === q.get('resourceId'))
	);
}

function analytics(q) {
	const rows = filterLog(q);
	const countries = new Map();
	const days = new Map();
	let blocked = 0;
	for (const r of rows) {
		if (!r.action) blocked++;
		const c = countries.get(r.location) ?? { code: r.location, count: 0, blockedCount: 0 };
		c.count++;
		if (!r.action) c.blockedCount++;
		countries.set(r.location, c);
		const day = new Date(r.timestamp * 1000).toISOString().slice(0, 10);
		const d = days.get(day) ?? { day, totalCount: 0, blockedCount: 0 };
		d.totalCount++;
		if (!r.action) d.blockedCount++;
		days.set(day, d);
	}
	return {
		totalRequests: rows.length,
		totalBlocked: blocked,
		totalAllowed: rows.length - blocked,
		requestsPerCountry: [...countries.values()],
		requestsPerDay: [...days.values()].sort((a, b) => a.day.localeCompare(b.day))
	};
}

function page(items, q) {
	const size = Number(q.get('pageSize') ?? q.get('limit') ?? 1000);
	const offset = q.get('page') ? (Number(q.get('page')) - 1) * size : Number(q.get('offset') ?? 0);
	return items.slice(offset, offset + size);
}

/** Routes: [method, regex, handler(match, query, body)] → data, or { status, message }. */
const routes = [
	['GET', /^\/v1\/?$/, () => ({ message: 'Healthy' })],
	[
		'GET',
		/^\/v1\/orgs$/,
		(_m, _q, _b, key) =>
			key.endsWith('.scoped')
				? { status: 403, message: 'Key does not have permission to list organizations' }
				: {
						orgs: [
							{ orgId: ORG, name: 'Acme Corp' },
							{ orgId: 'lab', name: 'Lab' }
						]
					}
	],
	['GET', /^\/v1\/org\/([\w-]+)$/, (m) => ({ org: { orgId: m[1], name: 'Acme Corp' } })],
	['GET', /^\/v1\/org\/[\w-]+\/logs\/analytics$/, (_m, q) => analytics(q)],
	[
		'GET',
		/^\/v1\/org\/[\w-]+\/logs\/request$/,
		(_m, q) => {
			const rows = filterLog(q);
			return {
				log: page(rows, q),
				pagination: {
					total: rows.length,
					limit: Number(q.get('limit') ?? 1000),
					offset: Number(q.get('offset') ?? 0)
				}
			};
		}
	],
	['GET', /^\/v1\/org\/[\w-]+\/sites$/, (_m, q) => ({ sites: page(db.sites, q) })],
	[
		'GET',
		/^\/v1\/org\/[\w-]+\/pick-site-defaults$/,
		() => ({
			exitNodeId: 1,
			subnet: '100.89.128.12/30',
			newtId: 'newt' + id(),
			newtSecret: 'secret-' + Math.random().toString(36).slice(2),
			clientAddress: '100.90.128.9/24',
			publicKey: 'SrvPubKey0000000000000000000000000000000000=',
			endpoint: 'pangolin.example.com',
			listenPort: 51820
		})
	],
	[
		'PUT',
		/^\/v1\/org\/[\w-]+\/site$/,
		(_m, _q, b) => {
			if (!b.name || !['newt', 'wireguard', 'local'].includes(b.type))
				return { status: 400, message: 'Invalid site' };
			const site = {
				siteId: id(),
				niceId: `site-${nextId}`,
				name: b.name,
				type: b.type,
				online: false,
				subnet: b.subnet ?? '',
				address: b.address ?? '',
				pubKey: b.pubKey ?? ''
			};
			db.sites.push(site);
			return site;
		}
	],
	['DELETE', /^\/v1\/site\/(\d+)$/, (m) => del(db.sites, 'siteId', m[1])],
	['GET', /^\/v1\/org\/[\w-]+\/site-resources$/, (_m, q) => ({ siteResources: page(db.siteResources, q) })],
	[
		'PUT',
		/^\/v1\/org\/[\w-]+\/site-resource$/,
		(_m, _q, b) => {
			if (
				!b.name ||
				!b.mode ||
				!Array.isArray(b.userIds) ||
				!Array.isArray(b.roleIds) ||
				!Array.isArray(b.clientIds)
			)
				return { status: 400, message: 'Invalid site resource' };
			const row = {
				...b,
				siteResourceId: id(),
				siteId: b.siteIds?.[0] ?? b.siteId,
				siteName: db.sites.find((s) => s.siteId === (b.siteIds?.[0] ?? b.siteId))?.name
			};
			db.siteResources.push(row);
			return row;
		}
	],
	[
		'POST',
		/^\/v1\/site-resource\/(\d+)$/,
		(m, _q, b) =>
			patch(db.siteResources, 'siteResourceId', m[1], { ...b, siteId: b.siteIds?.[0] ?? b.siteId })
	],
	['DELETE', /^\/v1\/site-resource\/(\d+)$/, (m) => del(db.siteResources, 'siteResourceId', m[1])],
	[
		'GET',
		/^\/v1\/site-resource\/(\d+)\/(users|roles|clients)$/,
		(m) => {
			const row = db.siteResources.find((r) => String(r.siteResourceId) === m[1]);
			const key = { users: 'userId', roles: 'roleId', clients: 'clientId' }[m[2]];
			return { [m[2]]: (row?.[`${key}s`] ?? []).map((v) => ({ [key]: v })) };
		}
	],
	['GET', /^\/v1\/org\/[\w-]+\/resources$/, (_m, q) => ({ resources: page(db.resources, q) })],
	[
		'PUT',
		/^\/v1\/org\/[\w-]+\/resource$/,
		(_m, _q, b) => {
			if (!b.name || (b.http ? !b.domainId : !b.proxyPort))
				return { status: 400, message: 'Invalid resource' };
			const domain = db.domains.find((d) => d.domainId === b.domainId)?.baseDomain;
			const row = {
				...b,
				resourceId: id(),
				enabled: true,
				ssl: true,
				fullDomain: b.http ? [b.subdomain, domain].filter(Boolean).join('.') : undefined
			};
			db.resources.push(row);
			return row;
		}
	],
	[
		'POST',
		/^\/v1\/resource\/(\d+)$/,
		(m, _q, b) => {
			const row = db.resources.find((r) => String(r.resourceId) === m[1]);
			if (row?.http)
				b.fullDomain = [
					b.subdomain ?? row.subdomain,
					db.domains.find((d) => d.domainId === (b.domainId ?? row.domainId))?.baseDomain
				]
					.filter(Boolean)
					.join('.');
			return patch(db.resources, 'resourceId', m[1], b);
		}
	],
	['DELETE', /^\/v1\/resource\/(\d+)$/, (m) => del(db.resources, 'resourceId', m[1])],
	[
		'GET',
		/^\/v1\/resource\/(\d+)\/targets$/,
		(m) => ({ targets: db.targets.filter((t) => String(t.resourceId) === m[1]) })
	],
	[
		'PUT',
		/^\/v1\/resource\/(\d+)\/target$/,
		(m, _q, b) => {
			if (!b.siteId || !b.ip || !b.port) return { status: 400, message: 'Invalid target' };
			const row = { ...b, targetId: id(), resourceId: Number(m[1]), enabled: true };
			db.targets.push(row);
			return row;
		}
	],
	['POST', /^\/v1\/target\/(\d+)$/, (m, _q, b) => patch(db.targets, 'targetId', m[1], b)],
	['DELETE', /^\/v1\/target\/(\d+)$/, (m) => del(db.targets, 'targetId', m[1])],
	['GET', /^\/v1\/org\/[\w-]+\/domains$/, () => ({ domains: db.domains })],
	['GET', /^\/v1\/org\/[\w-]+\/users$/, (_m, q) => ({ users: page(db.users, q) })],
	['GET', /^\/v1\/org\/[\w-]+\/roles$/, (_m, q) => ({ roles: page(db.roles, q) })],
	[
		'PUT',
		/^\/v1\/org\/[\w-]+\/role$/,
		(_m, _q, b) => {
			if (!b.name) return { status: 400, message: 'Name is required' };
			const row = { ...b, roleId: id(), isAdmin: false };
			db.roles.push(row);
			return row;
		}
	],
	['POST', /^\/v1\/role\/(\d+)$/, (m, _q, b) => patch(db.roles, 'roleId', m[1], b)],
	[
		'DELETE',
		/^\/v1\/role\/(\d+)$/,
		(m, _q, b) =>
			b.roleId
				? del(db.roles, 'roleId', m[1])
				: { status: 400, message: 'roleId (the role to move members to) is required' }
	],
	['GET', /^\/v1\/org\/[\w-]+\/idp$/, () => ({ idps: db.idps })],
	['GET', /^\/v1\/org\/[\w-]+\/invitations$/, () => ({ invitations: db.invitations })],
	[
		'POST',
		/^\/v1\/org\/[\w-]+\/create-invite$/,
		(_m, _q, b) => {
			if (!b.email || !b.validHours) return { status: 400, message: 'Invalid invitation' };
			const invite = {
				inviteId: 'inv' + id(),
				email: b.email,
				expiresAt: Date.now() + b.validHours * 3_600_000,
				roles: (b.roleIds ?? []).map((r) => ({
					roleId: r,
					roleName: db.roles.find((x) => x.roleId === r)?.name
				}))
			};
			db.invitations.push(invite);
			return {
				inviteLink: `https://pangolin.example.com/invite?token=${invite.inviteId}-abcdef`,
				expiresAt: invite.expiresAt
			};
		}
	],
	['DELETE', /^\/v1\/org\/[\w-]+\/invitations\/([\w-]+)$/, (m) => del(db.invitations, 'inviteId', m[1])],
	['GET', /^\/v1\/org\/[\w-]+\/user-devices$/, () => ({ devices: db.devices })],
	['GET', /^\/v1\/org\/[\w-]+\/clients$/, () => ({ clients: db.clients })],
	[
		'POST',
		/^\/v1\/client\/(\d+)\/(block|unblock|archive|unarchive)$/,
		(m) => {
			const field = m[2].includes('block') ? 'blocked' : 'archived';
			return patch(db.devices, 'clientId', m[1], { [field]: !m[2].startsWith('un') });
		}
	],
	['DELETE', /^\/v1\/client\/(\d+)$/, (m) => del(db.devices, 'clientId', m[1])],
	['GET', /^\/v1\/org\/[\w-]+\/access-tokens$/, () => ({ accessTokens: db.accessTokens })],
	['DELETE', /^\/v1\/access-token\/([\w-]+)$/, (m) => del(db.accessTokens, 'accessTokenId', m[1])]
];

function del(items, key, value) {
	const index = items.findIndex((x) => String(x[key]) === String(value));
	if (index < 0) return { status: 404, message: 'Not found' };
	items.splice(index, 1);
	return null;
}

function patch(items, key, value, changes) {
	const row = items.find((x) => String(x[key]) === String(value));
	if (!row) return { status: 404, message: 'Not found' };
	Object.assign(row, Object.fromEntries(Object.entries(changes).filter(([, v]) => v !== undefined)));
	return row;
}

createServer((req, res) => {
	const chunks = [];
	req.on('data', (c) => chunks.push(c));
	req.on('end', () => {
		const url = new URL(req.url, 'http://x');
		const send = (status, payload) => {
			res.writeHead(status, { 'content-type': 'application/json' });
			res.end(JSON.stringify(payload));
			console.log(status, req.method, url.pathname + url.search);
		};
		const key = (req.headers.authorization ?? '').replace(/^Bearer /, '');
		if (!key || key.startsWith('wrong'))
			return send(401, { data: null, success: false, error: true, message: 'Unauthorized', status: 401 });
		let body;
		try {
			body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString()) : {};
		} catch {
			return send(400, { data: null, success: false, error: true, message: 'Invalid JSON', status: 400 });
		}
		for (const [method, pattern, handler] of routes) {
			const match = req.method === method && pattern.exec(url.pathname);
			if (!match) continue;
			const result = handler(match, url.searchParams, body, key);
			if (result && result.status && result.message)
				return send(result.status, {
					data: null,
					success: false,
					error: true,
					message: result.message,
					status: result.status
				});
			return send(200, { data: result, success: true, error: false, message: 'OK', status: 200 });
		}
		send(404, {
			data: null,
			success: false,
			error: true,
			message: `No route for ${req.method} ${url.pathname}`,
			status: 404
		});
	});
}).listen(PORT, '127.0.0.1', () =>
	console.log(`Mock Pangolin API on http://127.0.0.1:${PORT} (org "${ORG}")`)
);
