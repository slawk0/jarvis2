#!/usr/bin/env bash
# Rebuild the debug binary and restart it against an isolated config dir.
# Usage: dev/relaunch.sh <config-dir>
set -e
cd "$(dirname "$0")/.."
taskkill //F //IM jarvis-server-manager.exe >/dev/null 2>&1 || true
(cd src-tauri && cargo build 2>&1 | grep -E "^(error|warning: unused)|Finished" -A8)
powershell -ExecutionPolicy Bypass -File dev/run-app.ps1 -ConfigDir "$(cygpath -w "$1")" | tail -1
