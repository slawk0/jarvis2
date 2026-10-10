# Developer guide

How Jarvis Server Manager is built and how to work on it. The commands and types that cross the
frontend/backend boundary are listed in the [IPC reference](ipc-reference.md).

- [Setup](#setup)
- [Commands](#commands)
- [Architecture](#architecture)
- [Backend](#backend)
- [Frontend](#frontend)
- [Security model](#security-model)
- [Local storage](#local-storage)
- [Adding a feature](#adding-a-feature)
- [Testing](#testing)
- [CI and releases](#ci-and-releases)
- [Conventions](#conventions)

## Setup

Prerequisites: Node 22+, pnpm 11, Rust (stable) and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your platform.

```sh
pnpm install
JARVIS_CONFIG_DIR=/tmp/jarvis-dev pnpm tauri dev
```

Always set `JARVIS_CONFIG_DIR` while developing. It points the app at a separate data directory, so
a development build never reads your real profiles, and you never connect to a real server by
accident. Use the [test server](#testing) as the target instead.

## Commands

| Command                                     | What it does                                                        |
| ------------------------------------------- | ------------------------------------------------------------------- |
| `pnpm tauri dev`                            | Run the app with hot reload                                         |
| `pnpm check`                                | Type-check the frontend (svelte-check)                              |
| `pnpm lint`                                 | ESLint and Prettier check                                           |
| `pnpm format`                               | Format the frontend with Prettier                                   |
| `pnpm test`                                 | Frontend unit tests (Vitest)                                        |
| `pnpm bindings`                             | Regenerate `src/lib/ipc/bindings.ts` from the Rust commands         |
| `pnpm docs:ipc`                             | Regenerate `docs/ipc-reference.md` from the bindings                |
| `pnpm version:sync [x.y.z]`                 | Copy the version from `package.json` into `Cargo.toml`/`Cargo.lock` |
| `pnpm tauri build`                          | Production bundles                                                  |
| `cargo test`                                | Backend unit tests (run inside `src-tauri/`)                        |
| `cargo clippy --all-targets -- -D warnings` | Backend lints                                                       |
| `cargo fmt --check`                         | Backend formatting                                                  |

After changing a Rust command or a type that crosses the IPC boundary, run `pnpm bindings`, then
`pnpm check`, then `pnpm docs:ipc`.

## Architecture

Jarvis is a Tauri 2 app: a Rust backend and a SvelteKit static SPA (Svelte 5 runes, TypeScript,
Tailwind 4, shadcn-svelte) in a system webview.

```
src/                      SvelteKit SPA (no SSR)
  routes/                 the single page: login screen or workspace, plus global dialogs
  lib/ipc/                generated command bindings, the api wrapper, error mapping
  lib/services/           app state, sudo, jobs, transfers, settings, toasts, dialogs, updater
  lib/state/              resource / polling helpers shared by every tab
  lib/components/         shared UI: DataTable, Modal, LogViewer, DependencyGuard, …
  lib/editor/             Monaco-based code and file editors
  lib/screens/            login, profile form, settings, host-key and update dialogs
  lib/workspace/          pane layout engine, tab registry, sidebar
  lib/features/<tab>/     one folder per tab
src-tauri/src/            Rust backend, one module per domain
  ssh/                    connection, host keys, session (exec, sudo, tunnels, SFTP)
  store/                  profiles, per-profile documents, settings, keyring secrets
  sftp/                   the transfer engine
  jobs.rs                 streamed long-running commands
  shell.rs                quoting, command builder, input validators
  <domain>.rs             typed Tauri commands: docker, nginx, firewall, backups, …
dev/                      test server, mock Pangolin API, UI-driving helpers
scripts/                  version sync, IPC reference generator
```

The central rule: **the frontend never builds shell commands.** It calls typed commands
(`api.dockerContainers()`, `api.backupRun(id)`). Each backend module validates and quotes its
arguments, runs the command over SSH and parses the output into typed data. The exceptions are the
features whose purpose is running the user's own command: Terminal, Runbooks, cron job commands and
`docker exec`.

A request flows like this:

1. A tab calls `api.someCommand(args)` (`src/lib/ipc`).
2. Tauri invokes the `#[tauri::command]` of that name. It takes the active `Session` from
   `AppState`, or fails with `NOT_CONNECTED`.
3. The command builds a script with `Cmd`/`q()` and runs it with `session.exec(...)`.
4. The output is parsed by a plain function in the same module and returned as a typed value, or the
   command returns an `AppError`.
5. `api` turns a rejected call into an `IpcError`. On `SUDO_PASSWORD_REQUIRED` it opens the sudo
   dialog and retries the call.

## Backend

Everything is under `src-tauri/src`.

### Entry point

`lib.rs` registers every command and event in `specta_builder()`, sets up the plugins, the tray icon
and `AppState`. Closing the main window hides it; the app keeps running in the tray.

`export_bindings()` writes `src/lib/ipc/bindings.ts`. It runs on every debug start and in
`cargo test export_bindings`, which is what `pnpm bindings` calls.

### Errors

`error.rs` defines the one error type that crosses the IPC boundary: `AppError { code: ErrorCode,
details }`. Commands return `AppResult<T>`. A new failure mode gets a new `ErrorCode`; the frontend
maps every code to a message in `src/lib/ipc/errors.ts` through a `Record<ErrorCode, string>`, so a
missing mapping is a compile error.

### Shell safety

`shell.rs`:

- `q(value)` POSIX-single-quotes one shell word.
- `Cmd` builds a command line from separately quoted arguments. Its `lit` method takes only
  `&'static str`, so raw shell syntax can come from source code but never from runtime data.
- `validate::*` checks user input with a known shape: names, path components, host names, ports and
  port ranges, cron expressions and so on.

Every value that reaches a shell goes through one of these.

### SSH session

`ssh/session.rs`: `Session` is the connection to the active server.

- `exec(Exec)` runs a POSIX shell script and returns an `Output`. `run`, `run_sudo`, `exec_auto` and
  `run_auto` are shorthands; the `*_auto` variants retry with sudo when the failure is a permission
  problem.
- Scripts run as `env LC_ALL=C LANG=C sh -c '…'` with the sbin directories added to `PATH`, so
  parsers see untranslated output and tools are found for non-login shells.
- The main connection allows 6 concurrent commands (below OpenSSH's default `MaxSessions` of 10).
- `lease_stream()` gives long-lived streams (jobs, log follows, transfers) a channel on pooled side
  connections, so they never starve short commands.
- `sftp()` is the shared SFTP client, `tunnel()` opens a `direct-tcpip` channel (used by the
  database browser).
- Each terminal gets its own SSH connection (`terminal/`).

`ssh/client.rs` opens and authenticates a connection (15 s connect timeout, 20 s authentication
timeout). `ssh/known_hosts.rs` is an OpenSSH-style known-hosts file in the app config directory.

`connection.rs` owns the lifecycle. A monitor pings the server every 5 seconds; after two failed
pings it reports `offline` and reconnects with exponential backoff capped at 30 seconds, emitting a
`ConnectionStatus` event for every state change.

### Sudo

`sudo.rs` is the only sudo implementation. `Exec::sudo()` marks a command as needing root.

- The mode is detected once per session: root, passwordless, password or unavailable.
- The password is written to the channel's stdin for `sudo -S`. It never appears on a command line.
- A verified password is cached in memory for 15 minutes.
- Five wrong passwords lock further attempts for 60 seconds.
- Without a cached password an elevated command fails with `SUDO_PASSWORD_REQUIRED`; the frontend
  prompts and retries.

### Jobs

`jobs.rs` is the one mechanism for long-running, streamed operations.

- `state.jobs.start(app, session, JobMeta, Exec)` streams one remote command.
- `state.jobs.spawn(...)` runs a custom async body with a `JobCtx` (`stream`, `step`, `note`).
- A job emits `JobStarted`, then `JobOutput` chunks, then one `JobDone`.
- A _visible_ job is listed in the Running Jobs panel; a _hidden_ one (a log follow, live stats) is
  only seen by the view that started it.
- Cancelling kills the remote process group (TERM, then KILL), not just the local listener.

### Dependencies

`deps.rs` holds the `Tool` enum with, per tool, a probe and an install recipe for each package
manager (apt, dnf, yum, apk, pacman, zypper). `deps::require(session, tool)` fails with
`DEPENDENCY_MISSING`. `deps_install` installs a tool as a streamed job.

### Domain modules

One module per domain: `docker/`, `systemd.rs`, `cron.rs`, `disks.rs`, `packages.rs`, `users.rs`,
`env.rs`, `nginx.rs`, `network.rs`, `firewall.rs`, `crowdsec.rs`, `files.rs`, `db/`, `backups.rs`,
`restic.rs`, `logs.rs`, `stats.rs`, `runbooks.rs`, `terminal/`. Each keeps its command builders and
output parsers as plain functions with unit tests next to them, and thin `#[tauri::command]`
wrappers at the bottom.

Where a tool offers a machine format (`--json`, `--format`, `-o json`), the module parses that
instead of human-readable output.

Notable designs:

- **Files** (`files.rs`) run as shell commands, not SFTP, so the same code path works as the login
  user and as root. Bulk byte transfer is in `sftp/transfer.rs`: a queue with 3 concurrent jobs, up
  to 3 attempts each, `.part` files and an atomic replace.
- **Databases** (`db/`) use native MySQL and PostgreSQL drivers through an SSH tunnel bound to
  `127.0.0.1:<ephemeral port>`; no client is needed on the server. `db/sql.rs` builds the SQL text.
- **Backups** (`backups.rs`) generate one POSIX shell script that serves both "run now" and the
  schedule. It reads its secrets from the environment.
- **Nginx** (`nginx.rs`) stores a proxy host as one generated config file whose first line carries
  the settings as JSON. Saving writes the file, runs `nginx -t` and rolls back on failure.
- **Docker** (`docker/spec.rs`) recreates a container with a rename → stop → create → start → remove
  script that restores the old container on any failure.
- **Pangolin** (`pangolin.rs`) is an HTTP proxy to the Pangolin API, not SSH. The frontend builds
  the API paths; the backend adds the key from the keyring and accepts only plain `/v1/...` paths.

### IPC types (specta)

Types that cross the boundary derive `specta::Type`. Constraints that bite:

- No `serde_json::Value` in IPC types: pass JSON as a `String`.
- No `#[serde(default | alias | flatten | skip_serializing_if)]` on types that derive `Type`.
- `f64` fields need `#[specta(type = i32)]`, otherwise they are exported as `number | null`.

## Frontend

Everything is under `src/lib`.

### IPC

`ipc/bindings.ts` is generated. `ipc/index.ts` exports two wrappers around it:

- `api.*` opens the sudo dialog on `SUDO_PASSWORD_REQUIRED` and retries.
- `apiQuiet.*` never prompts. Use it for polling, so a background refresh cannot pop up a dialog.

Both reject with an `IpcError` (`code`, `details`, `title`, `is(...)`).

### State helpers

`state/resource.svelte.ts`:

- `resource(loader)` gives `data`, `error`, `loading` and `refresh`.
- `autoLoad(res, () => visible, pollMs)` loads on first visibility and optionally polls.
- `poll(...)` for custom polling.
- `Busy` tracks per-row busy state.

Polling stops while a tab is hidden or the server is offline.

### Services

`services/` holds the app-wide singletons: `app` (session, link status, profiles), `sudo`, `jobs`,
`transfers`, `settings`, `toast`, `confirm`/`prompt`, `updater` and `profile-data`
(`loadDoc`/`saveDoc`, typed per-profile documents).

### Components

Use the shared components in `components/` instead of new one-offs: `Page`, `SubTabs`, `DataTable`
(sort, search, select, bulk actions, context menu, virtualised), `Modal`, `StateView`, `Field`,
`SelectField`, `LogViewer`, `JobDialog`, `DependencyGuard`, `PathInput`, `DirPicker`, `CronInput`,
`IpLink`, `RefreshControl`, `TargetProfiles`. `components/ui/` is shadcn-svelte: generated, and
excluded from Prettier.

### Workspace

- `workspace/layout.ts` is the pure split-tree engine (unit-tested).
- `workspace/workspace.svelte.ts` is the store: panes, focus, back stack, and the cross-tab
  `request()` bus (for example "look this IP up in Net Diagnostics").
- `workspace/registry.ts` is the tab list that drives the sidebar, the pane tab picker and pane
  rendering.

### Tab contract

A tab is a component in `features/<tab>/` that:

- takes `{ visible, profile }` props,
- loads on first `visible` and pauses polling while it is not visible,
- may export `refresh()` (called by the refresh buttons) and `onReselect()` (the active tab was
  clicked again: return to the root view).

Tabs stay mounted in their pane after the first visit, so terminals and log views survive switching.

### Svelte pitfalls seen in this codebase

- A variable named `state` breaks `$state`.
- A component must not share its name with a `.svelte.ts` module in the same folder.
- A `$derived` that returns the same array or object reference does not notify; return a copy.
- Writing state inside an `$effect` that also reads it loops; wrap the write in `untrack`, or build
  the value in a local first.
- `{@const}` must be the direct child of a block.

## Security model

- **Host keys** are checked against the app's own known-hosts file. An unknown or changed key stops
  the connection until the user decides.
- **Secrets** (SSH passwords, key passphrases, database passwords, API keys) live only in the OS
  keyring. The UI can set, test for and clear a secret; no command reads one back.
- **Secrets on the wire** reach remote programs through the environment or stdin, never through argv
  or a world-readable file. Backups and restic send `export NAME='value'` lines on stdin and
  `eval "$(cat)"` them. A scheduled backup keeps them in `/etc/jarvis-backups/<id>.env` (mode 600).
- **The sudo password** travels only on a channel's stdin and is held in memory for 15 minutes.
- **Shell input** is always quoted or validated (see [Shell safety](#shell-safety)).
- **The webview** never sees the Pangolin API key or any other secret.
- **Updates** are signed; the public key is in `src-tauri/tauri.conf.json`.
- **Release notes** and other remote text are rendered as plain text, never as HTML.

## Local storage

`store/` persists everything as JSON files in the app config directory, written atomically (temp
file, fsync, rename). A file that cannot be parsed is moved aside, never overwritten, and reported
once through `startup_notices`.

| Path             | Content                                                     |
| ---------------- | ----------------------------------------------------------- |
| `profiles.json`  | Server profiles (no secrets)                                |
| `settings.json`  | App settings and UI preferences                             |
| `known_hosts`    | Trusted host keys, OpenSSH layout                           |
| `profiles/<id>/` | One JSON document per `DataKey`, plus the secret-name index |
| `global/`        | The secret-name index of secrets not tied to a profile      |

`DataKey` lists the per-profile documents: `runbooks`, `savedCommands`, `sftpBookmarks`,
`backupTemplates`, `resticRepos`, `alertThresholds`, `dbConnections`, `nginxTargets`,
`logAnalysisProfiles`, `logSources`, `crowdsecConfig`, `composeStacks`, `workspaceLayout`. Documents
the backend acts on are read into typed structs; documents only the UI cares about are stored as-is
and typed on the TypeScript side.

Secrets are in the OS keyring under the service `com.jarvis.servermanager`, with names like
`ssh/password` or `db/<id>/password`. The keyring cannot be enumerated portably, so each owner keeps
an index of its secret names; that index is what makes "delete a profile and all of its secrets"
possible.

`profiles.json` and the keyring entries of Jarvis v1 are read as they are, so an existing install is
picked up.

## Adding a feature

### A new command

1. Add the `#[tauri::command]` (with `#[specta::specta]`) to the domain module, with its builder and
   parser as plain, unit-tested functions.
2. Register it in `specta_builder()` in `lib.rs`.
3. Run `pnpm bindings`, `pnpm check` and `pnpm docs:ipc`.

### A new error code

Add it to `ErrorCode` in `error.rs`, run `pnpm bindings`, and add its message to
`src/lib/ipc/errors.ts` (the type checker insists).

### A new tab

1. A backend module with its commands, registered in `lib.rs`; `pnpm bindings`.
2. `features/<tab>/<Tab>.svelte` following the [tab contract](#tab-contract).
3. An entry in `workspace/registry.ts`, with `requires: [...]` when the whole tab needs server tools.
4. A section in [tabs.md](tabs.md).

### A new server tool

Add it to the `Tool` enum in `deps.rs` with a probe and the package names per package manager, then
either list it in a tab's `requires` or wrap the part that needs it in `DependencyGuard`.

## Testing

Unit tests live next to the code: parsers and command builders in each Rust module, the layout
engine, cron and formatting helpers on the frontend.

```sh
pnpm check && pnpm lint && pnpm test
cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

`cargo test` takes one filter.

### Test server

`dev/test-server/` is a disposable Ubuntu container with sshd, systemd, sudo, Docker access, nginx,
ufw, cron and restic, plus MariaDB and PostgreSQL containers. Its locale is `pl_PL.UTF-8`, to prove
the parsers do not depend on English output. See
[dev/test-server/README.md](../dev/test-server/README.md) for the accounts and setup.

The backend's end-to-end tests run against it and are `#[ignore]`d, so a plain `cargo test` stays
offline:

```sh
cd src-tauri
JARVIS_TEST_KEY=~/.ssh/jarvis_test cargo test live_ -- --ignored --test-threads=4
```

The test server mounts the host's Docker socket, so the Docker tab shows every container on your
machine. Do not prune or delete anything you did not create for testing.

### Other helpers

- `dev/mock-pangolin.mjs`: an in-memory stand-in for the Pangolin API.
- `dev/run-app.ps1` (Windows): starts the debug build against an isolated data directory with the
  WebView's DevTools protocol on port 9222. `dev/cdp.mjs` takes screenshots and clicks through it.
- `dev/relaunch.sh`: rebuilds the debug binary and restarts it.

## CI and releases

`.github/workflows/ci.yml` runs on pushes to `main` and on pull requests: `pnpm check`, `pnpm lint`
and `pnpm test` for the frontend; `cargo fmt --check`, `cargo clippy` and `cargo test` for the
backend.

The version lives in `package.json`. `tauri.conf.json` reads it from there, and `pnpm version:sync`
copies it into `Cargo.toml` and `Cargo.lock`.

```sh
pnpm version:sync 1.2.3
git commit -am "Release 1.2.3"
git tag v1.2.3
git push --follow-tags
```

Pushing a `v*` tag (or starting the workflow by hand) runs `.github/workflows/release.yml`, which
checks that the versions are in sync, builds Windows (NSIS, MSI) and Linux (deb, rpm, AppImage)
bundles, signs the updater artifacts and publishes a GitHub Release including `latest.json`.

Add an entry to [CHANGELOG.md](../CHANGELOG.md) with every release.

The updater needs the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; the matching public key and the update endpoint are in
`src-tauri/tauri.conf.json` under `plugins.updater`.

## Conventions

- Everything in the repository is in English.
- Destructive actions go through `confirm({ destructive: true })`; the riskiest use `typeToConfirm`.
- Wording: "Delete" destroys data on the server, "Remove" only forgets something in Jarvis. Sentence
  case everywhere.
- Parse machine formats where a tool offers them.
- No user names, hosts or other personal data in code, tests or fixtures.
- Comments explain why, not what.
