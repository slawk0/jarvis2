# Test server

A disposable server for exercising Jarvis end to end: an Ubuntu 24.04 container running systemd
and sshd, with sudo, cron, nginx, ufw, restic, rclone and database clients installed, plus
MariaDB and PostgreSQL containers next to it.

```sh
docker compose -f dev/test-server/docker-compose.yml up -d --build
```

| What       | Value                                                                      |
| ---------- | -------------------------------------------------------------------------- |
| SSH        | `127.0.0.1:2222`                                                           |
| `admin`    | password `jarvis-test`; sudo asks for the password                         |
| `keyuser`  | key login only; passwordless sudo                                          |
| MariaDB    | container `jarvis-test-mysql`, user `shop` / `shoppw`, database `shop`     |
| PostgreSQL | container `jarvis-test-postgres`, user `app` / `pgpw`, database `appdb`    |
| Locale     | `pl_PL.UTF-8` by default, to prove parsers do not depend on English output |

## Key login

`keyuser` accepts the public keys listed in `dev/test-server/authorized_keys` (git-ignored).
Create a key pair and add its public half before starting the container:

```sh
ssh-keygen -t ed25519 -N '' -f ~/.ssh/jarvis_test
cat ~/.ssh/jarvis_test.pub >> dev/test-server/authorized_keys
```

## The Docker tab

The container mounts the **host's** Docker socket so the Docker tab has something to manage.
That means it shows, and can change, every container, image and volume on your machine. Do not
prune or delete anything you did not create for testing.

## Backend end-to-end tests

They are `#[ignore]`d so a plain `cargo test` stays offline:

```sh
cd src-tauri
JARVIS_TEST_KEY=~/.ssh/jarvis_test cargo test live_ -- --ignored --test-threads=4
```

## Driving the UI

On Windows, `dev/run-app.ps1` starts the debug build against an isolated data directory with
the WebView's DevTools protocol on port 9222 (`pnpm dev` must be running), and `dev/cdp.mjs`
takes screenshots and clicks through it:

```sh
node dev/cdp.mjs shot out.png
node dev/cdp.mjs text "Connect"
```

## Reset

```sh
docker compose -f dev/test-server/docker-compose.yml down -v
```
