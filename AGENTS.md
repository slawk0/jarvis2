# Notes for coding agents

Jarvis Server Manager is a desktop app (Tauri 2) that manages Linux servers over SSH. Rust backend
in `src-tauri/`, SvelteKit static SPA (Svelte 5 runes, TypeScript, Tailwind 4, shadcn-svelte) in
`src/`. Everything in the repository is in English.

Read [docs/development.md](docs/development.md) before a non-trivial change: it describes the
architecture, the building blocks to reuse and the security model. [CLAUDE.md](CLAUDE.md) is the
condensed version of the same notes.

## Commands

```sh
pnpm tauri dev                 # run the app
pnpm check && pnpm lint && pnpm test
pnpm bindings                  # regenerate src/lib/ipc/bindings.ts after any IPC change
pnpm docs:ipc                  # regenerate docs/ipc-reference.md after pnpm bindings
cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

`cargo test` takes one filter. Live tests against `dev/test-server` are `#[ignore]`d:
`JARVIS_TEST_KEY=<private key> cargo test live_ -- --ignored --test-threads=4`.

## Rules that must not be broken

- Set `JARVIS_CONFIG_DIR=<dir>` for every development run, so the app does not read the user's real
  profiles. Never connect to servers from the real profile list; use `dev/test-server`.
- Every value that reaches a remote shell goes through `shell::q()`, `shell::Cmd` or a
  `shell::validate::*` function. The frontend never sends shell text, except where the feature is
  running the user's own command (Terminal, Runbooks, cron job commands).
- Secrets live only in the OS keyring (`store/secrets`). They reach remote programs through the
  environment or stdin, never through argv or a world-readable file.
- `sudo.rs` is the only sudo implementation. Mark a command with `Exec::sudo()`; do not build `sudo`
  command lines yourself.
- Never edit `src/lib/ipc/bindings.ts`, `docs/ipc-reference.md` or `src/lib/components/ui/` by hand;
  they are generated.
- No user names, hosts or other personal data in code, tests or fixtures.

## Where things go

- A backend feature: its domain module in `src-tauri/src/`, with command builders and output parsers
  as plain unit-tested functions and thin `#[tauri::command]` wrappers at the bottom. Register the
  command in `specta_builder()` in `lib.rs`.
- A new failure mode: a new `ErrorCode` in `error.rs` and its message in `src/lib/ipc/errors.ts`.
- A long-running operation: a job (`jobs.rs`), not a blocking command.
- A tab: `src/lib/features/<tab>/`, taking `{ visible, profile }` props and exporting `refresh()`,
  plus an entry in `src/lib/workspace/registry.ts`.
- UI: reuse `src/lib/components/` (`Page`, `DataTable`, `Modal`, `StateView`, `Field`, `LogViewer`,
  `DependencyGuard`, …) before writing a one-off.
- Polling: `apiQuiet.*`, which never opens the sudo dialog. Everything else: `api.*`.

## IPC type constraints (specta)

- No `serde_json::Value`: pass JSON as a `String`.
- No `#[serde(default | alias | flatten | skip_serializing_if)]` on types that derive `Type`.
- `f64` fields need `#[specta(type = i32)]`.

## Conventions

- Keep changes simple: no abstractions or options for a future that has not arrived.
- Destructive actions go through `confirm({ destructive: true })`; the riskiest use `typeToConfirm`.
- "Delete" destroys data on the server, "Remove" only forgets something in Jarvis. Sentence case.
- Parse machine formats (`--json`, `--format`, `-o json`) where a tool offers them.
- Update [docs/tabs.md](docs/tabs.md) when a tab's behaviour changes, and
  [CHANGELOG.md](CHANGELOG.md) with every release.
