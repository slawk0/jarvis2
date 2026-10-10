# User guide

Jarvis Server Manager is a desktop app for managing Linux servers over SSH. Nothing is installed on
the server: Jarvis runs ordinary commands over SSH and shows their results. This guide covers the
parts of the app that are the same on every tab. Each tab is described in [tabs.md](tabs.md).

- [Installing](#installing)
- [Server profiles](#server-profiles)
- [Connecting](#connecting)
- [The workspace](#the-workspace)
- [Root privileges (sudo)](#root-privileges-sudo)
- [Missing tools on the server](#missing-tools-on-the-server)
- [Running jobs](#running-jobs)
- [File transfers](#file-transfers)
- [Tables, logs and editors](#tables-logs-and-editors)
- [Settings](#settings)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Updates](#updates)
- [Where your data is stored](#where-your-data-is-stored)
- [Troubleshooting](#troubleshooting)

## Installing

Download the installer for your system from the project's GitHub Releases page:

| System  | Packages                     |
| ------- | ---------------------------- |
| Windows | NSIS installer (`.exe`), MSI |
| Linux   | `.deb`, `.rpm`, AppImage     |

On Linux the app needs a Secret Service provider (GNOME Keyring, KWallet or KeePassXC) to store
passwords; see [Troubleshooting](#troubleshooting).

### What the server needs

- Linux with an SSH server and a POSIX shell. Jarvis is tested on Ubuntu and Debian and avoids
  GNU-only flags where an alternative exists.
- Password or private-key login (OpenSSH and PEM keys, with or without a passphrase).
- `sudo` for anything that needs root, unless you connect as root.
- Per tab, the tool that tab manages: systemd, cron, Docker (with the compose plugin), nginx, certbot,
  ufw or iptables, CrowdSec, restic, rclone. Jarvis offers to install a missing tool.

The server's language does not matter: every command runs with `LC_ALL=C`.

## Server profiles

A profile holds the connection details of one server. The start screen lists your profiles as cards.

Choose **New profile** (or **Create first profile**) and fill in:

| Field            | Notes                                                                                  |
| ---------------- | -------------------------------------------------------------------------------------- |
| Label            | The name shown in the app.                                                             |
| Host, Port       | Host name or IP address; the port defaults to 22.                                      |
| User             | The account to log in as; defaults to `root`.                                          |
| Authentication   | **Password** or **Private key**.                                                       |
| Password         | Stored in the system keyring.                                                          |
| Private key file | Path to an OpenSSH or PEM private key. Picking a `.pub` file selects its private half. |
| Key passphrase   | Only if the key is encrypted. Stored in the system keyring.                            |

When editing a profile, leave the password or passphrase empty to keep the stored one, or tick
**Clear the stored password/passphrase from the system keyring** to forget it.

Hover a card for its actions:

- **Star**: connect to this profile automatically when Jarvis starts. Only one profile can be the
  default.
- **Edit**.
- **Delete**: removes the profile, its stored credentials and everything saved for that server
  (runbooks, bookmarks, backup templates, …) from this computer. The server itself is not touched.

## Connecting

Click a profile card to connect.

### Host keys

Jarvis keeps its own list of trusted host keys, separate from `~/.ssh/known_hosts`.

- **First connection**: the server's key fingerprint is shown. Compare it with the server's real
  fingerprint and choose **Trust and connect**.
- **The key changed**: Jarvis shows the previously trusted and the new fingerprint and refuses to
  continue until you tick the confirmation and choose **Replace key and connect**. A changed key can
  mean the server was reinstalled, or that the connection is being intercepted.

Trusted keys are listed under **Settings → Known hosts**, where a host can be forgotten.

### Connection status

The bottom of the sidebar shows the active server and its status: **Online**, **Offline**,
**Reconnecting…** or **Switching…**.

Jarvis checks the connection every 5 seconds. When it is lost, Jarvis reconnects by itself, waiting a
little longer after each failed attempt (up to 30 seconds). **Reconnect** retries immediately. While
the server is offline, tabs stop refreshing.

From the same place you can copy the host name, switch to another profile (**Switch server**) or
**Disconnect**.

## The workspace

After connecting you see the sidebar with all tabs, grouped by category, and the workspace on the
right.

### Sidebar

- Click a tab to open it in the focused pane. Type in **Search tabs…** to filter the list.
- Right-click a tab to **Open in new pane**, **Add to favourites** (favourites are pinned to the top)
  or give it a colour tag.
- Drag a tab into the workspace to drop it into a pane.
- Click the logo, or press `Ctrl+Alt+B`, to collapse the sidebar to icons.

### Panes

The workspace holds up to four panes, each showing one tab. The layout is saved per server.

- The buttons in the top bar switch between **Single pane**, **Side by side**, **Stacked** and a
  **2 × 2 grid**.
- With several panes open, each pane has a header to choose its tab, **Refresh**, **Split
  vertically**, **Split horizontally** and **Close pane**. Drag the grip on the left of the header to
  move a pane, and drag the border between panes to resize them.
- A tab that was opened in a pane stays alive in the background when you switch away, so a terminal
  or a log view keeps running. Hidden tabs do not poll the server.

The top bar also has **Back** (also the mouse back button), **Refresh active tab**, **Running jobs**,
**Keyboard shortcuts** and **Settings**.

## Root privileges (sudo)

Jarvis runs commands as the user you connected with and asks for root only when an action needs it.

- If you connect as root, or the user has passwordless sudo, nothing is ever asked.
- Otherwise the **Administrator password required** dialog appears, naming the action it is needed
  for. Enter the user's sudo password and the action continues automatically.
- The password is kept in memory for 15 minutes and is never written to disk. After that the dialog
  appears again.
- Five wrong passwords lock the dialog for 60 seconds.
- If the user has no sudo rights, actions that need root fail with a message saying so.

Many read-only views work without root and simply show less. For example, **Network / Ports** hides
the process names of other users until you choose **Show all processes**. The file browser reads
folders you cannot access as root and marks them.

## Missing tools on the server

A tab that needs a tool the server does not have shows a **Missing requirements** screen instead of
an error. For each tool it offers:

- **Install …**: installs it with the server's package manager (apt, dnf, yum, apk, pacman or zypper)
  and shows the live output.
- The exact command, to copy and run yourself.
- **Docs**: the tool's documentation.
- **Re-check**, after installing by hand.

Some things cannot be installed by Jarvis (systemd, for example); the screen says so.

## Running jobs

Long operations run as jobs: package upgrades, image pulls, compose actions, certificate requests,
backups, runbooks, partitioning and so on. A job shows its live output where you started it, and is
also listed in the **Running jobs** panel (the list icon in the top bar, with a badge for the number
of running jobs).

In the panel you can open a job to read its command and output, filter, copy or download the
output, **Stop** it (the remote process is terminated, not just the view) and **Clear finished**
jobs. Jobs are kept until you clear them or close the app.

## File transfers

Uploads, downloads and large move, copy and delete operations go through a transfer queue shown in
the **Transfers** panel.

- Three transfers run at a time; the rest wait in the queue. A failed transfer is retried
  automatically up to three times.
- A file is written under a temporary `.part` name and replaces the target only when it is complete.
- If targets already exist you are asked once for the whole batch: **Skip existing**, **Keep both**
  (the new file gets a number in its name) or **Overwrite**.
- Files your user may not read or write are transferred as root; they are marked with a shield icon.
- The panel can **Cancel** one transfer or all of them, **Retry failed** and **Clear completed**.

Downloads go to the folder set in **Settings → General → Default download folder** unless an action
asks for a folder.

## Tables, logs and editors

The same building blocks are used on every tab.

**Tables.** Click a column header to sort. Most tables have a search box. Tick rows to act on several
at once; the bulk actions appear below the table. Right-click a row for its full menu.

**Refresh.** Tabs load when first shown. Many have a refresh button with an optional **Auto-refresh**
interval.

**Log and output views.** Filter lines, pause and resume following, clear, jump to the end, copy
everything or download it as a file.

**Editors.** Configuration and text files open in a code editor. `Ctrl+S` saves. A file that could
only be read as root is marked and is saved as root. Closing with unsaved changes asks first. Files
larger than 5 MB and binary files are not opened.

**IP addresses.** Right-click an IP address anywhere in the app to look it up in **Net Diagnostics**.

**Confirmations.** Anything that destroys data asks first. Jarvis uses **Delete** for actions that
destroy something on the server and **Remove** for actions that only make Jarvis forget something.

## Settings

Open Settings from the gear icon (top bar or start screen).

| Section     | What it holds                                                                                                                                                                                     |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| General     | **Theme** (dark, light, follow system) and the **Default download folder**.                                                                                                                       |
| Terminal    | Font, size, line height, letter spacing, colour theme, cursor, scrollback, bell, and clipboard and mouse behaviour. Changes apply to open terminals immediately.                                  |
| Alerts      | Only while connected: desktop notifications for this server when CPU, RAM or disk usage stays above a threshold (defaults 95 %, 90 %, 85 %; at most one notification every 5 minutes per metric). |
| Known hosts | The trusted host keys, with **Forget host**.                                                                                                                                                      |
| About       | The version and **Check for updates**.                                                                                                                                                            |

Settings apply to the whole app, except alerts, which are saved per server.

## Keyboard shortcuts

Press `Ctrl+Shift+H` to see this list in the app.

| Keys                | Action                           |
| ------------------- | -------------------------------- |
| `Ctrl+N`            | Split the focused pane           |
| `Ctrl+W`            | Close the focused pane           |
| `Ctrl+1` … `Ctrl+4` | Focus pane 1–4 (reading order)   |
| `Ctrl+Tab`          | Next tab                         |
| `Ctrl+Shift+Tab`    | Previous tab                     |
| `Ctrl+Shift+T`      | Open Terminal                    |
| `Ctrl+Alt+B`        | Toggle sidebar                   |
| `Ctrl+Shift+H`      | Show keyboard shortcuts          |
| `Ctrl+Shift+C`      | Copy in terminal                 |
| `Ctrl+Shift+V`      | Paste in terminal                |
| `Ctrl+S`            | Save in editors                  |
| `Ctrl+Enter`        | Run the script in the SQL editor |
| Mouse back button   | Go back                          |
| `Esc`               | Close the topmost dialog or menu |

## Updates

Jarvis looks for a new version shortly after it starts and from **Settings → About → Check for
updates**. When one is available it shows the release notes; **Update now** downloads and installs
it and restarts the app, **Later** postpones it. Updates are signed, and an update whose signature
does not match is not installed.

Closing the window does not quit Jarvis: it keeps running in the system tray with its connection
open. Click the tray icon to show the window again, or use **Quit** in the tray menu.

## Where your data is stored

Everything stays on your computer.

| What                                                   | Where                                                                              |
| ------------------------------------------------------ | ---------------------------------------------------------------------------------- |
| Profiles (`profiles.json`), settings (`settings.json`) | The app config directory                                                           |
| What Jarvis remembers per server (`profiles/<id>/`)    | The app config directory                                                           |
| Trusted host keys (`known_hosts`)                      | The app config directory                                                           |
| Passwords, key passphrases, API keys and tokens        | The operating system keyring (Windows Credential Manager, Secret Service on Linux) |

The app config directory is `%APPDATA%\com.jarvis.servermanager` on Windows and
`~/.config/com.jarvis.servermanager` on Linux.

Per server, Jarvis remembers runbooks, saved terminal commands, file bookmarks, backup templates,
restic repositories, database connections, alert thresholds, nginx targets, log-analysis profiles,
custom log files, the CrowdSec connection, known compose stacks and the workspace layout.

Secrets are never written to these files, and the app cannot display a stored secret again: you can
only replace or clear it.

A few features leave files **on the server**, and only when you use them:

| Feature                    | Files on the server                                                                                                                                                            |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Scheduled backups          | `/usr/local/bin/jarvis-backup-<id>.sh`, `/etc/jarvis-backups/<id>.env` (mode 600, holds the backup's secrets), `/var/log/jarvis-backup-<id>.log` and a block in root's crontab |
| Nginx proxy hosts          | One `jarvis-…` config file per proxy host under the nginx configuration directory                                                                                              |
| DNS-challenge certificates | The DNS provider credentials under `/etc/letsencrypt/jarvis` (mode 600)                                                                                                        |
| Services and timers        | Unit files in `/etc/systemd/system`, marked `# Managed by Jarvis Server Manager`                                                                                               |
| Persistent variables       | Lines marked `# jarvis-managed` in `~/.bashrc`, `~/.profile`, `~/.bash_profile` or `/etc/environment`                                                                          |
| CrowdSec whitelist         | `jarvis-whitelist.yaml` in `/etc/crowdsec/parsers/s02-enrich`                                                                                                                  |

If a local data file is damaged, Jarvis moves it aside instead of overwriting it and tells you at
startup.

## Troubleshooting

**"Authentication failed."** Check the user name and the password or key in the profile. For a key,
make sure the public half is in the server's `authorized_keys`.

**"This private key is encrypted."** Enter the passphrase in the profile.

**"The system keyring could not be accessed."** On Linux a Secret Service provider must be running
and unlocked (GNOME Keyring, KWallet with the Secret Service bridge, or KeePassXC). On a minimal
desktop, install and unlock `gnome-keyring`.

**"This action needs root, but sudo is not available for this user."** Add the user to the sudo
group on the server, or connect as a user that has sudo.

**"A required tool is not installed on the server."** Open the tab that owns the feature and use its
install button; see [Missing tools on the server](#missing-tools-on-the-server).

**"The configuration test failed; the change was rolled back."** nginx rejected the configuration
you saved. The previous configuration is back in place; the message contains nginx's own error.

**Blank or flickering window on Linux.** Jarvis sets `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` at startup
unless the variable is already set. If the window is still blank, start Jarvis with
`WEBKIT_DISABLE_COMPOSITING_MODE=1`.

**No tray icon on Linux.** Install `libayatana-appindicator3`; on GNOME also the AppIndicator
extension. Without a tray, closing the window still hides it; start Jarvis again to bring the window
back.

**The AppImage does not start.** It needs FUSE 2 (`libfuse2`), or run it with
`--appimage-extract-and-run`.
