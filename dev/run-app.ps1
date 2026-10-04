# Start the debug build against an isolated data directory, with the WebView's
# DevTools protocol exposed for dev/cdp.mjs. Needs `pnpm dev` running (port 1420).
#
#   pwsh dev/run-app.ps1 [-ConfigDir <path>] [-Port 9222]
param(
	[string]$ConfigDir = (Join-Path $env:TEMP 'jarvis-dev-config'),
	[int]$Port = 9222
)

$exe = Join-Path $PSScriptRoot '..\src-tauri\target\debug\jarvis-server-manager.exe'
if (-not (Test-Path $exe)) {
	throw "Build first: cargo build --manifest-path src-tauri/Cargo.toml"
}

Get-Process jarvis-server-manager -ErrorAction SilentlyContinue | Stop-Process -Force
New-Item -ItemType Directory -Force $ConfigDir | Out-Null

$env:JARVIS_CONFIG_DIR = $ConfigDir
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
Start-Process -FilePath $exe
"Started with JARVIS_CONFIG_DIR=$ConfigDir, DevTools on port $Port"
