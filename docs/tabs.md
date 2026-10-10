# Tab reference

What each tab does, what it needs on the server and what it changes there. The parts shared by all
tabs (sudo, jobs, transfers, tables) are in the [user guide](user-guide.md).

| Category       | Tabs                                                                                                                                                                                                                |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Overview       | [Dashboard](#dashboard), [Terminal](#terminal), [Runbooks](#runbooks)                                                                                                                                               |
| System         | [Services](#services), [Docker](#docker), [Processes](#processes), [Systemd Timers](#systemd-timers), [Cron](#cron), [Disks](#disks), [Maintenance](#maintenance), [Users](#users), [Env Variables](#env-variables) |
| Network & Web  | [Nginx Manager](#nginx-manager), [Pangolin Proxy](#pangolin-proxy), [Network / Ports](#network--ports), [Net Diagnostics](#net-diagnostics)                                                                         |
| Security       | [Firewall](#firewall), [CrowdSec](#crowdsec)                                                                                                                                                                        |
| Data & Storage | [Files (SFTP)](#files-sftp), [Databases](#databases), [Backups](#backups), [Restic Backups](#restic-backups)                                                                                                        |
| Monitoring     | [Logs](#logs), [Log Analysis](#log-analysis)                                                                                                                                                                        |

## Overview

### Dashboard

A live summary of the server.

- Gauges for **CPU**, **Memory** and **Disk /**, and a **CPU and memory** chart of the recent history.
- Uptime, load average, swap, operating system, hostname and kernel.
- **Disk partitions** with usage per mount point.
- **Top processes by memory**, with a link to the full [Processes](#processes) tab.
- **Desktop alerts**: whether threshold notifications are on for this server; **Configure** opens
  [Settings → Alerts](user-guide.md#settings).
- **Pangolin · last 7 days**: requests, allowed, blocked, block rate and top countries, once the
  [Pangolin Proxy](#pangolin-proxy) tab is connected.

Everything is read from `/proc`, `df` and `ps`, so it also works on BusyBox systems.

### Terminal

Interactive shells on the server, in tabs inside the Terminal tab. Each terminal uses its own SSH
connection, so a busy terminal does not slow the rest of the app down.

- **New server shell** opens another login shell. A shell inside a Docker container is opened from
  the [Docker](#docker) tab (**Shell**).
- **Restart session** reconnects a terminal that ended.
- **Edit file** opens a file on the server in the editor. You can also select an absolute path in
  the terminal and choose **Open selected path in editor** from the context menu.
- **Saved commands** is a per-server list of commands you can run in the terminal with one click. It
  starts with a few examples (`df -h`, `free -m`, `docker ps`, …).
- **Open in external terminal** starts your operating system's terminal with an `ssh` session to the
  same server.
- **Terminal settings** opens [Settings → Terminal](user-guide.md#settings).

Copy with `Ctrl+Shift+C`, paste with `Ctrl+Shift+V`. Pasting several lines asks for confirmation,
because a shell runs pasted text line by line. Right-click, middle-click and `Ctrl+C`/`Ctrl+V`
behaviour are configurable.

Closing a terminal ends its shell and anything running in it.

### Runbooks

Named commands you run often, saved per server. Unlike a saved terminal command, a runbook runs as a
[job](user-guide.md#running-jobs): the output is streamed live and the exit code is shown.

- **New runbook**: a name, a command (run with `sh`; several lines are allowed) and whether it needs
  sudo.
- **Quick presets** add common ones: Docker prune, Compose status, Nginx test, reload and restart,
  Disk usage, Failed units.
- **Run** starts it, **Stop** terminates it.

Runbooks and the Terminal are the only places where Jarvis runs a command exactly as you typed it.

## System

### Services

Requires systemd.

Lists every systemd service with its state, whether it starts on boot, and its description.

- **Start**, **Stop**, **Restart**, **Reload**, **Enable on boot**, **Disable on boot**.
- **Status, logs and unit file** opens the detail view with three tabs: `systemctl status`, the
  unit's journal (followed live) and the unit file in an editor.
- Saving an edited unit file reloads systemd. A unit shipped by a package is not changed in place:
  your version is written to `/etc/systemd/system`, which takes precedence.
- **Create service** writes a new unit to `/etc/systemd/system`: name, description, command
  (`ExecStart`), user (empty runs as root), restart policy (on failure, always, never), working
  directory, environment (one `KEY=value` per line), and whether to enable and start it.
- Only units created by Jarvis can be deleted from here.

### Docker

Requires Docker. If the `docker` command needs root on the server, Jarvis detects that once and
uses sudo for it.

The tab follows Docker's event stream while it is visible, so changes made elsewhere (a terminal,
another tool) show up without a manual refresh.

**Containers.** Start, stop, restart, kill and remove, one at a time or in bulk. Per container:

- **Details**: image, command, restart policy, limits, ports, networks, mounts, environment and
  labels. From here you can rename the container, change its restart policy, join or leave networks,
  and run a one-off command inside it (**Exec**).
- **Logs**, followed live (last 100 to 10,000 lines).
- **Shell** opens a terminal inside the container.
- **Inspect** shows the `docker inspect` JSON.
- **Edit** changes settings that Docker cannot change on a running container: image, command,
  entrypoint, user, volumes, ports, networks, DNS, environment, labels, memory and CPU limits, log
  driver, capabilities and privileged mode. To apply them Jarvis **recreates** the container: the old
  one is renamed and stopped, the new one is created and started, and only then is the old one
  removed. If any step fails, the new container is removed and the old one is put back and
  restarted.

**Images.** Pull an image (with live progress), remove images, **Prune unused**, inspect. Images no
container uses are marked _unused_.

**Networks.** Create and remove user-defined networks, inspect.

**Volumes.** Remove volumes, **Prune unused**, inspect, and **Browse files** inside a volume (read
and written as root).

**Compose.** Lists compose projects. A project stays listed after it was brought down, so it can be
brought up again.

- **Up**, **Restart**, **Down**, **Pull images**, **Logs**.
- **Edit compose file** opens the file in the editor and validates it with `docker compose config`
  when you save. Run **Up** to apply the changes.
- **New project** creates a folder with a starter `docker-compose.yml`.
- **Attach external network…** sets the stack's default network to an existing Docker network.
- **Forget stack** removes it from the list only; its files and containers are not touched.

**Stats.** Live CPU, memory, network I/O, block I/O and process count per running container.

Removing a container deletes data that is not in a volume. Removing a volume deletes its data
permanently.

### Processes

All processes with PID, user, CPU %, memory %, nice value and command line. Search by command, user
or PID.

- **Terminate (SIGTERM)** asks the process to exit.
- **Kill (SIGKILL)** ends it immediately; it cannot clean up or save its state.
- **Change priority (renice)** sets the nice value, from −20 (highest priority) to 19 (lowest).

Acting on another user's process uses sudo automatically.

### Systemd Timers

Requires systemd.

Lists timers with their next and last run, whether they are enabled on boot, and their state.

- **Run now** starts the timer's service immediately.
- **Inspect** shows the status and unit files of the timer and its service.
- **Start timer**, **Stop timer**, **Enable on boot**, **Disable on boot**.
- **Create timer** creates a timer and the service it runs in `/etc/systemd/system`: name,
  description, schedule (a systemd `OnCalendar` expression such as `hourly`, `daily` or
  `Mon *-*-* 03:00:00`), command, user, and **Catch up after downtime** (run a missed job after
  boot).
- **Delete** is available for timers Jarvis created and removes the service with it.

Next and last run times need systemd 246 or newer.

### Cron

Requires cron.

- **Your crontab**: add, edit, enable or disable, and delete jobs. The schedule field offers presets
  and describes the expression in words. Jobs run as the connected user with `sh`.
- **root's crontab**: read-only. Backup schedules created by Jarvis live here and are managed on the
  [Backups](#backups) tab.
- **/etc/cron.d**: read-only view of the system cron files.

Jarvis changes only the line of the job you edit. Everything else in the crontab (variables,
comments, `@reboot` entries, blocks written by other tools) is kept exactly as it was.

### Disks

**Filesystem usage** per mount point. Switches show or hide system filesystems (tmpfs, overlay, …)
and loop devices.

**Block devices** lists disks and partitions with size, filesystem, mount point, label and UUID, and
a bar showing each disk's partition layout. Per device:

- **Mount…** to a directory (optionally creating it). The mount lasts until the next reboot; add an
  entry to `/etc/fstab` yourself to make it permanent.
- **Unmount**.
- **Expand to fill disk…** grows a partition and then its filesystem, for example after enlarging a
  virtual disk.
- **Check filesystem…** runs a filesystem check. A mounted filesystem is only inspected, never
  repaired.
- **Create partition** on a disk: in the largest unallocated area, or by erasing the disk with a
  new GPT or MBR table and one partition. The filesystem can be ext4, XFS or FAT32.

Partitioning operations ask for confirmation and run as jobs. **Erasing a disk destroys everything
on it.**

### Maintenance

Package management through the server's package manager: apt, dnf, yum, apk, pacman or zypper.

- **Updates**: **Refresh index** re-reads the package lists; **Upgrade all** installs every pending
  update. Services may restart during an upgrade.
- **Pending updates**: the list with installed and available versions.
- **Reboot**: shows whether the server asks for a reboot and why; **Reboot server…** restarts it.
  Jarvis reconnects when the server is back.
- **Automatic updates**: turns unattended (security) updates on or off, installing the mechanism for
  the distribution if needed.
- **Install or remove a package**: search the repositories, then **Install** or **Remove**. Removing
  a package may remove packages that depend on it.

### Users

**Users.** Lists accounts with UID, home, shell and groups. System accounts (UID below 1000) are
hidden unless you switch them on. Per user:

- **Group membership…**: tick the supplementary groups. Privileged groups such as `sudo` and
  `docker` are marked.
- **Change password…**: the password is sent to the server on standard input, never on a command
  line.
- **SSH authorized keys…**: edit the user's `authorized_keys`, one public key per line.
- **Lock account** / **Unlock account**.
- **Delete user…**: asks separately whether to delete the home directory. The account Jarvis is
  connected as cannot be deleted.

**Create user** takes a username, full name, shell and an optional password; a home directory is
created. Without a password the user can only log in with an SSH key.

**Groups.** Create and delete groups and see their members.

### Env Variables

- **Current environment**: what a login shell of the connected user gets. Read-only; values are
  hidden until you reveal them.
- **Persistent variables**: variables Jarvis manages in `~/.bashrc`, `~/.profile`,
  `~/.bash_profile` or `/etc/environment` (all users, needs root). **Add variable**, edit and
  **Remove**. A change applies to new login sessions.
- **Docker container**: the environment a container was configured with. Read-only.

Jarvis marks every line it writes with a `# jarvis-managed` comment and only ever edits those lines;
the rest of the file is left alone.

## Network & Web

### Nginx Manager

Manages nginx running on the server itself or inside a Docker container. The first time, tell Jarvis
where nginx runs and where its configuration directory is (usually `/etc/nginx`). Several targets
can be saved per server. Missing tools can only be installed automatically on the host.

**Proxy hosts.** Reverse-proxy hosts in the style of Nginx Proxy Manager.

- _Details_: domain names, scheme (`http` or `https` towards the application), forward host and
  port, **Websockets support**, **Block common exploits**, **Cache static assets**.
- _SSL_: enable HTTPS with a certificate from the SSL certificates tab, **Force HTTPS**, **HTTP/2**,
  **HSTS** (with subdomains and preload).
- _Advanced_: custom directives added inside the `location /` block.
- A switch enables or disables a host without deleting it.

Each proxy host is one configuration file written by Jarvis. Saving runs `nginx -t`; if the test
fails the file is rolled back and nothing changes. Other nginx configuration is left untouched.

**SSL certificates.** Let's Encrypt certificates managed by certbot.

- **Issue certificate**: domains, email and a validation method: the nginx plugin (recommended),
  webroot, standalone (port 80 must be free), or a DNS challenge through Cloudflare, DigitalOcean or
  AWS Route 53. Wildcard certificates need a DNS challenge.
- DNS provider tokens are stored in your system keyring and written to a root-only file on the
  server for certbot.
- **Renew**, **Force** renew, **Renew all**, **Dry run** (tests renewal without changing anything)
  and **Delete**.
- The list shows the domains, the expiry date and the days left.

**Config files.** Edit any configuration file under the nginx directory. Saving tests the
configuration and reloads nginx; a failed test restores the previous content.

**Control.** **Test configuration**, **Reload**, **Restart** and **Status**, with the command output.

### Pangolin Proxy

A client for the [Pangolin](https://github.com/fosrl/pangolin) Integration API. Unlike every other
tab it talks to an HTTP API from your computer, not to the server over SSH.

**Settings.** Enter the API URL (for a self-hosted Pangolin usually `https://api.your-domain`), an
API key created in Pangolin under Settings → API Keys, and the organisation. The key is stored only
in your system keyring. A key limited to one organisation cannot list organisations: type the
organisation ID.

**Dashboard.** Requests, allowed and blocked counts, traffic over time, a world map and the top
countries, for the last 24 hours, 7 days or 30 days. Filters by resource, action, country and host
are computed from the newest 5,000 log entries of the range.

**Logs.** The request audit log with a date range, filters (action, method, resource, country, host,
path, reason, actor), paging, and a detail view per request.

**Resources.**

- _Sites_: create Newt, basic WireGuard or local sites. The credentials of a new site are shown
  once; copy them then.
- _Private resources_: host, CIDR, HTTP or SSH resources reachable by clients, with allowed ports and
  the sites, roles, users and machine clients that may use them.
- _Public resources_: HTTP/HTTPS, SSH, RDP, VNC and raw TCP or UDP resources, with their domain or
  proxy port and their targets.

**Access.** Users and identity providers (read-only), roles (create, edit, delete with members
moved to another role) and invitations (create, copy the link, cancel).

**Clients.** User devices (block, archive, delete) and resource access tokens (revoke).

### Network / Ports

- **Listening**: listening sockets with protocol, address, port, process and PID. Without root the
  processes of other users are hidden; **Show all processes** reads the list as root.
- **Connections**: established TCP connections.
- **Interfaces**: network interfaces and their addresses.

### Net Diagnostics

Runs network checks **from the server**, with live output:

| Check      | Tool on the server | Target                  |
| ---------- | ------------------ | ----------------------- |
| Ping       | `ping`             | Host name or IP address |
| Traceroute | `traceroute`       | Host name               |
| DNS lookup | `dig`              | Name and record type    |
| HTTP check | `curl`             | URL                     |
| MTR        | `mtr`              | Host name               |
| Port check | `nc`               | Host name and port      |

A missing tool can be installed from the tab.

**IP info** shows country, region, city, provider, ASN, time zone and coordinates of an IP address.
This lookup is made from your computer to ipapi.co, not from the server.

## Security

### Firewall

Manages UFW and iptables. nftables and firewalld are detected and reported, but not managed.

**UFW.**

- **Enable** or **Disable** the firewall. Before enabling, Jarvis offers to allow the SSH port of
  your connection first, so you do not lock yourself out.
- **Add rule**: allow, deny (drop silently) or reject (answer with an error); protocol; port or
  range (`443`, `8000:8100`); source IP or CIDR.
- **Delete rule**.

**iptables.**

- Pick a table and a chain to see its rules and policy.
- **Add rule**: target (ACCEPT, DROP, REJECT), protocol, source, destination, destination port, and
  position (append, insert at the top, or at a line).
- **Delete rule**, and set the policy of a built-in chain.
- **Persist rules** saves the current rules so they survive a reboot. Rules added here are otherwise
  lost on reboot.
- **Raw output** shows the `iptables -L` listing.

Be careful with rules that affect SSH: a wrong rule can cut off your own connection.

### CrowdSec

Requires CrowdSec. Jarvis finds a native `cscli` or a running container with a CrowdSec image.
**Connection settings** selects the mode explicitly: detect automatically, installed on the server,
Docker container, or a custom command (for example `podman exec crowdsec cscli`).

- **Dashboard**: version and service state, with start, stop and restart for a native install.
- **Decisions**: active bans. **Add ban** for an IP or a range with a duration and reason;
  **Unban** one, or **Unban all** (which also lifts bans made by CrowdSec itself).
- **Whitelist**: IPs and ranges that are never banned. Entries added here go to a file managed by
  Jarvis; entries from other whitelist files are shown read-only. On CrowdSec 1.6.8 and newer,
  allowlists are shown and edited too.
- **Alerts**: recent alerts with source, scenario and event count; **Details** shows the full alert.
- **Bouncers**: **Register bouncer** returns an API key that is shown only once. Delete a bouncer or
  **Prune inactive** ones.
- **Metrics**: lines read and parsed per log source, with a preview of the latest lines of a file
  source.
- **Hub**: collections, parsers, scenarios and postoverflows. **Install**, **Upgrade**, **Remove**,
  and **Update hub** to refresh the index.

## Data & Storage

### Files (SFTP)

A file browser for the whole server, starting in your home folder.

- Navigate with the path bar (which completes paths as you type), **Parent folder** and
  **Bookmarks** (saved per server).
- The filter box filters the current folder; switch it to search file names in subfolders.
- Show or hide hidden files.
- **New file**, **New folder**, and **Upload** files or a whole folder. You can also drop files onto
  the list.
- Per item or selection: **Download**, **Move…**, **Copy to…**, **Duplicate…**, **Rename…**,
  **Compress** to `.tar.gz` or `.zip`, **Extract here**, **Permissions…**, **Owner / group…**,
  **Properties**, **Copy path** and **Delete**.
- Double-click a text file to edit it.

Folders and files your user cannot access are read and changed as root after the sudo prompt; the
browser marks them. Uploads, downloads and large operations run through the
[transfer queue](user-guide.md#file-transfers). Deleting is permanent: there is no trash.

### Databases

A browser for MySQL, MariaDB and PostgreSQL. The database is reached through the SSH connection, so
it does not have to be exposed to the internet and no client has to be installed on the server.

**Connections.** Saved per server; the password goes to the system keyring.

- _Host and port_: an address as seen from the server, for example `127.0.0.1:3306`.
- _Docker container_: pick a running container. Jarvis connects to the container's own IP address
  and can fill in engine, user, password and database from the container's environment.
- PostgreSQL needs a default database to log in.

**Browsing.** Choose a database (and a schema on PostgreSQL), then a table or view.

- **Data**: a paged grid, sorted and filtered on the server. Add conditions (`=`, `!=`, `<`, `>`,
  `<=`, `>=`, `LIKE`, `IS NULL`, `IS NOT NULL`). **Add row**, edit a row, delete selected rows (all
  in one transaction), and **Export** the table with the current filters to CSV or JSON.
- **Structure**: columns, indexes and foreign keys.
- **SQL editor**: write a script and run it with `Ctrl+Enter`. Statements run in order and stop at
  the first error; each one shows its result or the number of affected rows and the time it took.
  Results can be exported to CSV or JSON. A history of past scripts is kept.

Things to know:

- Views are read-only.
- A table without a primary key is edited by matching all columns of the row, so identical rows are
  changed together; the grid warns about this.
- A query result shows at most 5,000 rows, and very long cell values are cut.

### Backups

Backup templates for files and databases, run on demand or on a schedule.

**New backup** asks for:

- **What to back up**: a file or folder (packed into a `.tar.gz`), a MySQL/MariaDB database or a
  PostgreSQL database. A database can be reached by host and port or inside a Docker container; in a
  container the dump runs inside it, so no client is needed on the server.
- **Where to store it**:
  - a folder on the server,
  - this computer (you choose the folder on each run; cannot be scheduled),
  - S3-compatible storage (including Backblaze B2),
  - another server over SFTP,
  - a [restic repository](#restic-backups).
- **Retention**: delete archives older than a number of days (only this backup's own archives), or,
  for restic, keep-last/daily/weekly/monthly rules.
- **Schedule**: a cron expression, or none for a manual backup.

Each template is a card with **Run now**, a pause switch for its schedule, the **Schedule log**,
edit and delete. A card says so when the backup needs tools that are not installed, and offers to
install them.

How schedules work:

- A scheduled backup is installed **on the server** as a script, a root-only file with the secrets
  it needs, and an entry in root's crontab. It runs whether or not Jarvis is open.
- Pausing removes the crontab entry and keeps the script. Deleting the template removes all of it.
- The schedule log shows whether the script is installed, the crontab entry and the output of the
  last runs.

Passwords and keys are stored in your system keyring. They reach the backup tools through the
environment, never on a command line.

### Restic Backups

Requires restic (and rclone for rclone remotes).

Encrypted, deduplicated backups with [restic](https://restic.net/).

**Repositories.** Saved per server: a folder on the server, S3-compatible storage, Backblaze B2, an
SFTP server, a REST server or an rclone remote. The repository password, access keys and any extra
environment variables are stored in your system keyring. A new repository is created with
**Initialise repository**.

**Removing a repository from Jarvis** only forgets its configuration and secrets. The repository
and its data are not touched. Keep the repository password somewhere safe: without it the backups
cannot be read.

**Snapshots.**

- **Backup now**: paths (one absolute path per line), optional tags and exclude patterns.
- **Browse** a snapshot: navigate folders, search file names, preview text files (up to 1 MB) and
  download a file, or a folder as a `.tar` archive, to your computer.
- **Restore** a snapshot into a folder on the server. Files keep their full original paths below
  the target; use `/` to restore them to their original locations, which overwrites existing files.
- **Forget** selected snapshots. Their data is pruned and cannot be recovered.
- **Retention policy**: keep last/daily/weekly/monthly; **Dry run** shows what would be removed
  without changing anything.
- **Maintenance**: **Integrity check**, **Unlock** (remove stale locks) and **Prune unused data**.

## Monitoring

### Logs

**Log viewer.** Follows a log live.

- Sources: the systemd journal and the common log files found on the server. For the journal you
  can limit the view to a unit and a minimum priority.
- **Add log file** remembers another file for this server.
- Start from the last 100 to 10,000 lines.
- Logs your user cannot read are read as root after the sudo prompt.

**Sessions.**

- **Logged-in users**, with **Kick** to end another user's session (everything running in that
  terminal is killed). Terminals opened from Jarvis are marked and cannot be kicked from here.
- **Recent logins**.
- **Failed login attempts** (needs root).

### Log Analysis

Summarises a web server access log in the common or combined format.

Set up a profile: the web server (Nginx, Apache, Apache httpd or Traefik), whether it runs on the
server or in a Docker container, and the path of the access log. For a container the path can be
left empty to read the container's own log output.

Choose how many of the most recent lines to read (1,000 to 1,000,000) and press **Analyse** to see:

- the total number of requests,
- status codes and request methods,
- requests per hour,
- the top client IPs, paths and user agents.

The counting happens on the server; only the totals are sent back, so large logs are fine.
