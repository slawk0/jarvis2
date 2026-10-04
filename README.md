# Jarvis Server Manager

A desktop app for managing Linux servers over SSH. One window replaces a terminal, an SFTP
client, a Docker UI, a database browser and a handful of admin panels. Nothing is installed on
the server: Jarvis runs ordinary commands over SSH and parses their output.

Built with Tauri 2, Rust, SvelteKit (static SPA), Svelte 5 and Tailwind 4.

## Features

| Category       | Tabs                                                                                        |
| -------------- | ------------------------------------------------------------------------------------------- |
| Overview       | Dashboard, Terminal, Runbooks                                                               |
| System         | Services (systemd), Docker, Processes, Systemd Timers, Cron, Disks, Maintenance, Users, Env |
| Network & web  | Nginx Manager (proxy hosts, certbot), Pangolin Proxy, Network / Ports, Net Diagnostics      |
| Security       | Firewall (ufw, iptables), CrowdSec                                                          |
| Data & storage | Files (SFTP), Databases (MySQL/MariaDB, PostgreSQL), Backups, Restic Backups                |
| Monitoring     | Logs, Log Analysis                                                                          |

Across all tabs:

- **Workspace**: up to four panes, split and resized freely; the layout is saved per server.
- **Sudo on demand**: when a command needs root, Jarvis asks for the password once, keeps it in
  memory for 15 minutes and retries the action. It is sent over the SSH channel's stdin, never on
  a command line.
- **Secrets** (SSH passwords, key passphrases, database passwords, API keys) are stored only in
  the operating system keyring.
- **Long operations** run as jobs with live output in the Running Jobs panel and can be stopped.
- **Dependency guard**: a tab that needs a tool the server lacks (docker, restic, certbot, …)
  offers a one-click install with the server's package manager.
- **Host keys** are verified against the app's own `known_hosts`; unknown and changed keys are
  shown with their fingerprint before anything is sent.
- **Auto-update** with signed releases.

## Server requirements

- Linux with an SSH server and a POSIX shell. Tested on Ubuntu/Debian; the commands avoid
  GNU-only flags where an alternative exists.
- Password or key authentication (OpenSSH and PEM keys, with or without a passphrase).
- `sudo` for anything that needs root, unless you connect as root.
- Optional, per tab: systemd, cron, docker (+ compose plugin), nginx, certbot, ufw/iptables,
  CrowdSec, restic, rclone, `mysqldump`/`pg_dump` (only for backups of databases that do not run
  in a container).

The app forces `LC_ALL=C` for every command, so the server's language does not matter.

## Development

Prerequisites: Node 22+, pnpm 11, Rust (stable) and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your platform.

```sh
pnpm install
pnpm tauri dev
```

| Command                                     | What it does                                                |
| ------------------------------------------- | ----------------------------------------------------------- |
| `pnpm check`                                | Type-check the frontend (svelte-check)                      |
| `pnpm lint`                                 | ESLint and Prettier check                                   |
| `pnpm format`                               | Format the frontend with Prettier                           |
| `pnpm test`                                 | Frontend unit tests (Vitest)                                |
| `pnpm bindings`                             | Regenerate `src/lib/ipc/bindings.ts` from the Rust commands |
| `cargo test`                                | Backend unit tests (run inside `src-tauri/`)                |
| `cargo clippy --all-targets -- -D warnings` | Backend lints                                               |
| `pnpm tauri build`                          | Production bundles                                          |

After changing a Rust command or a type that crosses the IPC boundary, run `pnpm bindings` and
then `pnpm check`.

Set `JARVIS_CONFIG_DIR` to point a development build at a separate data directory, so it never
touches your real profiles:

```sh
JARVIS_CONFIG_DIR=/tmp/jarvis-dev pnpm tauri dev
```

### Test server

`dev/test-server/` is a disposable Ubuntu container with sshd, systemd, sudo, Docker access,
nginx, ufw, cron and restic, plus MariaDB and PostgreSQL containers. The backend's end-to-end
tests and manual UI checks run against it. See [dev/test-server/README.md](dev/test-server/README.md).

`dev/mock-pangolin.mjs` is an in-memory stand-in for the Pangolin API, for working on the
Pangolin tab without a Pangolin instance.

## Architecture

```
src/                      SvelteKit SPA (no SSR)
  lib/ipc/                generated command bindings + error mapping
  lib/services/           app state, sudo, jobs, transfers, settings, toasts, dialogs
  lib/state/              resource / polling helpers shared by every tab
  lib/components/         shared UI: DataTable, Modal, LogViewer, DependencyGuard, …
  lib/workspace/          pane layout engine, tab registry, sidebar
  lib/features/<tab>/     one folder per tab
src-tauri/src/            Rust backend, one module per domain
  ssh/                    connection, host keys, session (exec, sudo, tunnels, SFTP)
  store/                  profiles, per-profile documents, settings, keyring secrets
  jobs.rs                 streamed long-running commands
  <domain>.rs             typed Tauri commands: docker, nginx, firewall, backups, …
```

The frontend never builds shell commands. It calls typed commands (`api.dockerContainers()`,
`api.backupRun(id)`), and each backend module quotes its arguments, runs the command and parses
the result. Errors are a fixed set of codes that the frontend maps to messages. More detail is
in [CLAUDE.md](CLAUDE.md).

## Releases

The version lives in `package.json`; `tauri.conf.json` reads it from there and
`pnpm version:sync` copies it into `Cargo.toml` and `Cargo.lock`.

```sh
pnpm version:sync 1.2.3
git commit -am "Release 1.2.3"
git tag v1.2.3
git push --follow-tags
```

Pushing a `v*` tag (or starting the workflow by hand) runs `.github/workflows/release.yml`,
which builds Windows (NSIS, MSI) and Linux (deb, rpm, AppImage) bundles, signs the updater
artifacts and publishes a GitHub Release including `latest.json`.

Before the first release:

1. Add the repository secrets `TAURI_SIGNING_PRIVATE_KEY` (the contents of the minisign private
   key whose public key is in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`) and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (empty if the key has none). Generate a new pair with
   `pnpm tauri signer generate` if you do not have the private key.
2. Point `plugins.updater.endpoints` in `src-tauri/tauri.conf.json` at your repository:
   `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`.

## Where data is stored

| What                                | Where                                                                                                 |
| ----------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Profiles, settings, per-server data | The app config directory (`%APPDATA%\com.jarvis.servermanager`, `~/.config/com.jarvis.servermanager`) |
| Passwords, passphrases, API keys    | The OS keyring (Windows Credential Manager, Secret Service)                                           |
| Trusted host keys                   | `known_hosts` in the app config directory                                                             |

Deleting a profile removes its documents and its secrets.

## Linux troubleshooting

- **Blank or flickering window**: Jarvis sets `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` at startup
  unless the variable is already set. If the window is still blank, try
  `WEBKIT_DISABLE_COMPOSITING_MODE=1`.
- **"Keyring" errors when saving a password**: a Secret Service provider must be running
  (GNOME Keyring, KWallet with the Secret Service bridge, or KeePassXC). On a minimal desktop,
  install and unlock `gnome-keyring`.
- **No tray icon**: install `libayatana-appindicator3`; on GNOME also the AppIndicator
  extension. Without a tray, closing the window still hides it; start Jarvis again to bring the
  window back.
- **AppImage does not start**: it needs FUSE 2 (`libfuse2`), or run it with
  `--appimage-extract-and-run`.

## Credits

The world map in the Pangolin dashboard is from [@svg-maps/world](https://github.com/VictorCazanave/svg-maps)
(CC BY 4.0).
