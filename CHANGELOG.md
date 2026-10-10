# Changelog

All notable changes to Jarvis Server Manager. Versions follow [Semantic Versioning](https://semver.org/).

## 0.1.2 - 2026-10-09

### Changed

- The native title bar follows the app's light or dark theme. On Windows 11 it is painted in the
  app's background colour.

## 0.1.1 - 2026-10-04

### Fixed

- The restic repository dialog and the backup template dialog no longer fail to open with an error
  page.

## 0.1.0 - 2026-10-04

First release of the rewrite on Tauri 2, Rust and Svelte 5.

### Added

- Server profiles with password or private-key login, credentials in the system keyring, host-key
  verification, and automatic reconnection.
- A workspace with up to four resizable panes, favourites and colour tags, saved per server.
- Sudo on demand: the password is asked for once and kept in memory for 15 minutes.
- Running Jobs panel for long operations with live output, and a transfer queue for uploads and
  downloads.
- One-click installation of missing server tools with apt, dnf, yum, apk, pacman or zypper.
- Tabs: Dashboard, Terminal, Runbooks, Services, Docker (containers, images, networks, volumes,
  compose, stats), Processes, Systemd Timers, Cron, Disks, Maintenance, Users, Env Variables, Nginx
  Manager, Pangolin Proxy, Network / Ports, Net Diagnostics, Firewall (UFW, iptables), CrowdSec,
  Files (SFTP), Databases (MySQL/MariaDB, PostgreSQL), Backups, Restic Backups, Logs and Log
  Analysis.
- Terminal settings for appearance, cursor, clipboard and mouse behaviour, bell and scrollback.
- Signed automatic updates, with Windows (NSIS, MSI) and Linux (deb, rpm, AppImage) packages.
