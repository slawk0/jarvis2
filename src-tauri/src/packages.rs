//! Maintenance: pending updates, upgrades, automatic updates and reboot,
//! through a small abstraction over the common package managers.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};

use crate::deps::{package_manager, PackageManager};
use crate::error::{AppError, AppResult};
use crate::jobs::JobMeta;
use crate::shell::{q, validate};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdate {
    pub name: String,
    /// Installed version, when the package manager reports it.
    pub current: String,
    pub candidate: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AutoUpdates {
    Enabled,
    Disabled,
    /// The mechanism for this distribution is not installed (enabling installs it).
    NotInstalled,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceStatus {
    pub package_manager: Option<PackageManager>,
    pub updates: Vec<PendingUpdate>,
    pub reboot_required: bool,
    /// Packages or reasons that request the reboot.
    pub reboot_reasons: Vec<String>,
    pub auto_updates: AutoUpdates,
}

/// `apt list --upgradable`: `name/suite candidate arch [upgradable from: current]`.
pub fn parse_apt_upgradable(text: &str) -> Vec<PendingUpdate> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once('/')?;
            let mut fields = rest.split_whitespace();
            let _suite = fields.next()?;
            let candidate = fields.next()?.to_string();
            let current = line.split_once("from: ").map(|(_, v)| v.trim_end_matches(']').to_string()).unwrap_or_default();
            Some(PendingUpdate { name: name.to_string(), current, candidate })
        })
        .collect()
}

/// `dnf check-update` / `yum check-update`: `name.arch  version  repo`.
pub fn parse_dnf_check_update(text: &str) -> Vec<PendingUpdate> {
    text.lines()
        .take_while(|l| !l.starts_with("Obsoleting"))
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() != 3 || !f[0].contains('.') || !f[1].bytes().next()?.is_ascii_digit() {
                return None;
            }
            let name = f[0].rsplit_once('.').map(|(n, _)| n).unwrap_or(f[0]);
            Some(PendingUpdate { name: name.to_string(), current: String::new(), candidate: f[1].to_string() })
        })
        .collect()
}

/// `apk list -u`: `name-1.2.3-r0 arch {origin} (license) [upgradable from: name-1.2.2-r0]`.
pub fn parse_apk_upgradable(text: &str) -> Vec<PendingUpdate> {
    fn split_version(pkg: &str) -> (String, String) {
        // The version starts at the last `-` that is followed by a digit, before `-rN`.
        let base = pkg.rsplit_once("-r").map(|(b, _)| b).unwrap_or(pkg);
        match base.rfind('-') {
            Some(i) if base[i + 1..].starts_with(|c: char| c.is_ascii_digit()) => (pkg[..i].to_string(), pkg[i + 1..].to_string()),
            _ => (pkg.to_string(), String::new()),
        }
    }
    text.lines()
        .filter_map(|line| {
            let first = line.split_whitespace().next()?;
            let (name, candidate) = split_version(first);
            let current = line.split_once("from: ").map(|(_, v)| split_version(v.trim_end_matches(']')).1).unwrap_or_default();
            Some(PendingUpdate { name, current, candidate })
        })
        .collect()
}

/// `pacman -Qu`: `name current -> candidate`.
pub fn parse_pacman_upgradable(text: &str) -> Vec<PendingUpdate> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            (f.len() >= 4 && f[2] == "->").then(|| PendingUpdate {
                name: f[0].to_string(),
                current: f[1].to_string(),
                candidate: f[3].to_string(),
            })
        })
        .collect()
}

/// `zypper -q lu`: a table `S | Repository | Name | Current | Available | Arch`.
pub fn parse_zypper_upgradable(text: &str) -> Vec<PendingUpdate> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('|').map(str::trim).collect();
            (f.len() >= 5 && f[0] == "v").then(|| PendingUpdate {
                name: f[2].to_string(),
                current: f[3].to_string(),
                candidate: f[4].to_string(),
            })
        })
        .collect()
}

struct Commands {
    list: &'static str,
    refresh: &'static str,
    upgrade: &'static str,
}

fn commands(pm: PackageManager) -> Commands {
    match pm {
        PackageManager::Apt => Commands {
            list: "apt list --upgradable 2>/dev/null",
            refresh: "apt-get update",
            upgrade: "export DEBIAN_FRONTEND=noninteractive; apt-get -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold dist-upgrade",
        },
        PackageManager::Dnf => Commands {
            list: "dnf -q check-update 2>/dev/null; true",
            refresh: "dnf makecache --refresh",
            upgrade: "dnf -y upgrade",
        },
        PackageManager::Yum => Commands {
            list: "yum -q check-update 2>/dev/null; true",
            refresh: "yum makecache",
            upgrade: "yum -y update",
        },
        PackageManager::Apk => Commands {
            list: "apk list -u 2>/dev/null",
            refresh: "apk update",
            upgrade: "apk upgrade",
        },
        PackageManager::Pacman => Commands {
            list: "pacman -Qu 2>/dev/null; true",
            refresh: "pacman -Sy",
            upgrade: "pacman -Syu --noconfirm",
        },
        PackageManager::Zypper => Commands {
            list: "zypper -q lu 2>/dev/null; true",
            refresh: "zypper --non-interactive refresh",
            upgrade: "zypper --non-interactive update",
        },
    }
}

fn parse_updates(pm: PackageManager, text: &str) -> Vec<PendingUpdate> {
    match pm {
        PackageManager::Apt => parse_apt_upgradable(text),
        PackageManager::Dnf | PackageManager::Yum => parse_dnf_check_update(text),
        PackageManager::Apk => parse_apk_upgradable(text),
        PackageManager::Pacman => parse_pacman_upgradable(text),
        PackageManager::Zypper => parse_zypper_upgradable(text),
    }
}

const SECTION: &str = "#jarvis-section";

pub async fn status(session: &Session) -> AppResult<MaintenanceStatus> {
    let Some(pm) = package_manager(session).await? else {
        return Ok(MaintenanceStatus {
            package_manager: None,
            updates: Vec::new(),
            reboot_required: false,
            reboot_reasons: Vec::new(),
            auto_updates: AutoUpdates::Unsupported,
        });
    };
    let auto = match pm {
        PackageManager::Apt => {
            "if ! dpkg -s unattended-upgrades >/dev/null 2>&1; then echo not-installed; \
             elif apt-config dump APT::Periodic::Unattended-Upgrade 2>/dev/null | grep -q '\"1\"'; then echo enabled; else echo disabled; fi"
        }
        PackageManager::Dnf | PackageManager::Yum => {
            "if ! command -v dnf-automatic >/dev/null 2>&1 && ! systemctl cat dnf-automatic.timer >/dev/null 2>&1; then echo not-installed; \
             elif systemctl is-enabled dnf-automatic.timer >/dev/null 2>&1 || systemctl is-enabled dnf-automatic-install.timer >/dev/null 2>&1; then echo enabled; else echo disabled; fi"
        }
        _ => "echo unsupported",
    };
    let reboot = "if [ -f /var/run/reboot-required ]; then echo yes; cat /var/run/reboot-required.pkgs 2>/dev/null; \
                  elif command -v needs-restarting >/dev/null 2>&1 && ! needs-restarting -r >/dev/null 2>&1; then echo yes; \
                  else echo no; fi";
    let script = format!("{}; echo '{SECTION}'; {reboot}; echo '{SECTION}'; {auto}", commands(pm).list);
    let out = session.exec(Exec::new(script).secs(120)).await?.stdout;
    let parts: Vec<&str> = out.split(SECTION).collect();
    let part = |i: usize| parts.get(i).copied().unwrap_or("").trim();
    let mut reboot_lines = part(1).lines();
    let reboot_required = reboot_lines.next() == Some("yes");
    let mut reasons: Vec<String> = reboot_lines.map(str::to_string).filter(|l| !l.is_empty()).collect();
    reasons.sort();
    reasons.dedup();
    Ok(MaintenanceStatus {
        package_manager: Some(pm),
        updates: parse_updates(pm, part(0)),
        reboot_required,
        reboot_reasons: reasons,
        auto_updates: match part(2) {
            "enabled" => AutoUpdates::Enabled,
            "disabled" => AutoUpdates::Disabled,
            "not-installed" => AutoUpdates::NotInstalled,
            _ => AutoUpdates::Unsupported,
        },
    })
}

async fn manager(session: &Session) -> AppResult<PackageManager> {
    package_manager(session).await?.ok_or_else(|| AppError::unsupported("No supported package manager was found on this server"))
}

// ---------------------------------------------------------------- commands

#[tauri::command]
#[specta::specta]
pub async fn maintenance_status(state: State<'_, AppState>) -> AppResult<MaintenanceStatus> {
    let session = state.session()?;
    status(&session).await
}

/// Refresh the package index (streamed job).
#[tauri::command]
#[specta::specta]
pub async fn maintenance_refresh(app: AppHandle, state: State<'_, AppState>) -> AppResult<String> {
    let session = state.session()?;
    let command = commands(manager(&session).await?).refresh;
    let meta = JobMeta::visible("Refresh package index", command);
    state.jobs.start(&app, session, meta, Exec::new(command).sudo()).await
}

/// Upgrade all packages (streamed job).
#[tauri::command]
#[specta::specta]
pub async fn maintenance_upgrade(app: AppHandle, state: State<'_, AppState>) -> AppResult<String> {
    let session = state.session()?;
    let command = commands(manager(&session).await?).upgrade;
    let meta = JobMeta::visible("Upgrade all packages", command);
    state.jobs.start(&app, session, meta, Exec::new(command).sudo()).await
}

/// Turn automatic (security) updates on or off; installs the mechanism if needed.
#[tauri::command]
#[specta::specta]
pub async fn maintenance_auto_updates(app: AppHandle, state: State<'_, AppState>, enable: bool) -> AppResult<String> {
    let session = state.session()?;
    let script = match (manager(&session).await?, enable) {
        (PackageManager::Apt, true) => "export DEBIAN_FRONTEND=noninteractive; \
             dpkg -s unattended-upgrades >/dev/null 2>&1 || { apt-get update -q && apt-get install -y -q unattended-upgrades; } && \
             printf 'APT::Periodic::Update-Package-Lists \"1\";\\nAPT::Periodic::Unattended-Upgrade \"1\";\\n' > /etc/apt/apt.conf.d/20auto-upgrades && \
             echo 'Automatic updates enabled.'",
        (PackageManager::Apt, false) => "printf 'APT::Periodic::Update-Package-Lists \"0\";\\nAPT::Periodic::Unattended-Upgrade \"0\";\\n' > /etc/apt/apt.conf.d/20auto-upgrades && \
             echo 'Automatic updates disabled.'",
        (PackageManager::Dnf | PackageManager::Yum, true) => "{ command -v dnf-automatic >/dev/null 2>&1 || systemctl cat dnf-automatic.timer >/dev/null 2>&1 || dnf install -y dnf-automatic; } && \
             { systemctl enable --now dnf-automatic-install.timer 2>/dev/null || systemctl enable --now dnf-automatic.timer; } && echo 'Automatic updates enabled.'",
        (PackageManager::Dnf | PackageManager::Yum, false) => "systemctl disable --now dnf-automatic-install.timer dnf-automatic.timer 2>/dev/null; echo 'Automatic updates disabled.'",
        _ => return Err(AppError::unsupported("Automatic updates are not managed for this package manager")),
    };
    let title = if enable { "Enable automatic updates" } else { "Disable automatic updates" };
    state.jobs.start(&app, session, JobMeta::visible(title, script), Exec::new(script).sudo()).await
}

/// Reboot the server. Returns before the connection drops.
#[tauri::command]
#[specta::specta]
pub async fn maintenance_reboot(state: State<'_, AppState>) -> AppResult<()> {
    let session = state.session()?;
    session.run_sudo("(sleep 1; systemctl reboot 2>/dev/null || reboot) >/dev/null 2>&1 &").await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackageHit {
    pub name: String,
    pub description: String,
}

pub fn parse_search(pm: PackageManager, text: &str) -> Vec<PackageHit> {
    let hit = |name: &str, description: &str| PackageHit { name: name.trim().to_string(), description: description.trim().to_string() };
    match pm {
        // `apt-cache search`: "name - description"
        PackageManager::Apt => text.lines().filter_map(|l| l.split_once(" - ").map(|(n, d)| hit(n, d))).collect(),
        // "name.arch : description"
        PackageManager::Dnf | PackageManager::Yum => text
            .lines()
            .filter(|l| !l.starts_with('='))
            .filter_map(|l| l.split_once(" : "))
            .map(|(n, d)| hit(n.rsplit_once('.').map(|(a, _)| a).unwrap_or(n), d))
            .collect(),
        // `apk search -d`: "name-version - description"
        PackageManager::Apk => text.lines().filter_map(|l| l.split_once(" - ").map(|(n, d)| hit(n, d))).collect(),
        // `pacman -Ss`: "repo/name version" then an indented description line
        PackageManager::Pacman => {
            let mut hits = Vec::new();
            let mut lines = text.lines().peekable();
            while let Some(line) = lines.next() {
                if line.starts_with(' ') {
                    continue;
                }
                let name = line.split_whitespace().next().unwrap_or("");
                let name = name.split_once('/').map(|(_, n)| n).unwrap_or(name);
                let description = lines.next_if(|l| l.starts_with(' ')).unwrap_or("");
                if !name.is_empty() {
                    hits.push(hit(name, description));
                }
            }
            hits
        }
        // `zypper se`: table rows "S | Name | Summary | Type"
        PackageManager::Zypper => text
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split('|').map(str::trim).collect();
                (f.len() >= 4 && f[3] == "package").then(|| hit(f[1], f[2]))
            })
            .collect(),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn package_search(state: State<'_, AppState>, query: String) -> AppResult<Vec<PackageHit>> {
    let session = state.session()?;
    let query = query.trim();
    if query.len() < 2 || !query.bytes().all(|b| b.is_ascii_alphanumeric() || b"+-._ ".contains(&b)) {
        return Err(AppError::invalid("Enter at least two letters or digits"));
    }
    let pm = manager(&session).await?;
    let term = q(query);
    let script = match pm {
        PackageManager::Apt => format!("apt-cache search -- {term} | head -n 200"),
        PackageManager::Dnf => format!("dnf -q search {term} 2>/dev/null | head -n 200"),
        PackageManager::Yum => format!("yum -q search {term} 2>/dev/null | head -n 200"),
        PackageManager::Apk => format!("apk search -d -- {term} | head -n 200"),
        PackageManager::Pacman => format!("pacman -Ss -- {term} | head -n 400"),
        PackageManager::Zypper => format!("zypper -q se -- {term} | head -n 200"),
    };
    let out = session.exec(Exec::new(script).secs(90)).await?;
    Ok(parse_search(pm, &out.stdout))
}

/// Install or remove a single package (streamed job).
#[tauri::command]
#[specta::specta]
pub async fn package_change(app: AppHandle, state: State<'_, AppState>, name: String, install: bool) -> AppResult<String> {
    let session = state.session()?;
    validate::name("package name", &name)?;
    let pm = manager(&session).await?;
    let pkg = q(&name);
    let script = match (pm, install) {
        (pm, true) => {
            // `install_command` only takes static names; this one is validated and quoted above.
            pm.install_command(&[]).trim_end().to_string() + " " + &pkg
        }
        (PackageManager::Apt, false) => format!("export DEBIAN_FRONTEND=noninteractive; apt-get remove -y {pkg}"),
        (PackageManager::Dnf, false) => format!("dnf remove -y {pkg}"),
        (PackageManager::Yum, false) => format!("yum remove -y {pkg}"),
        (PackageManager::Apk, false) => format!("apk del {pkg}"),
        (PackageManager::Pacman, false) => format!("pacman -R --noconfirm {pkg}"),
        (PackageManager::Zypper, false) => format!("zypper --non-interactive remove {pkg}"),
    };
    let title = format!("{} {name}", if install { "Install" } else { "Remove" });
    state.jobs.start(&app, session, JobMeta::visible(title, script.clone()), Exec::new(script).sudo()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apt_upgradable() {
        let text = "Listing... Done\n\
curl/noble-updates,noble-security 8.5.0-2ubuntu10.6 amd64 [upgradable from: 8.5.0-2ubuntu10.4]\n\
libssl3t64/noble-updates 3.0.13-0ubuntu3.5 amd64 [upgradable from: 3.0.13-0ubuntu3.4]\n";
        let u = parse_apt_upgradable(text);
        assert_eq!(u.len(), 2);
        assert_eq!(u[0], PendingUpdate { name: "curl".into(), current: "8.5.0-2ubuntu10.4".into(), candidate: "8.5.0-2ubuntu10.6".into() });
        assert!(parse_apt_upgradable("Listing... Done\n").is_empty());
    }

    #[test]
    fn dnf_check_update() {
        let text = "\nkernel.x86_64                 5.14.0-427.el9          baseos\n\
openssl-libs.x86_64           1:3.0.7-27.el9          baseos\n\
Obsoleting Packages\nfoo.noarch 1.0 repo\n";
        let u = parse_dnf_check_update(text);
        assert_eq!(u.len(), 2);
        assert_eq!((u[1].name.as_str(), u[1].candidate.as_str()), ("openssl-libs", "1:3.0.7-27.el9"));
    }

    #[test]
    fn apk_pacman_zypper() {
        let apk = parse_apk_upgradable("busybox-1.36.1-r20 x86_64 {busybox} (GPL-2.0-only) [upgradable from: busybox-1.36.1-r15]\nlibcrypto3-3.3.2-r1 x86_64 {openssl} (Apache-2.0) [upgradable from: libcrypto3-3.3.1-r0]\n");
        assert_eq!((apk[0].name.as_str(), apk[0].current.as_str(), apk[0].candidate.as_str()), ("busybox", "1.36.1-r15", "1.36.1-r20"));
        assert_eq!(apk[1].name, "libcrypto3");

        let pac = parse_pacman_upgradable("linux 6.9.1.arch1-1 -> 6.9.3.arch1-1\nvim 9.1.0-1 -> 9.1.4-1\n");
        assert_eq!((pac[1].name.as_str(), pac[1].candidate.as_str()), ("vim", "9.1.4-1"));

        let zyp = parse_zypper_upgradable("S | Repository | Name | Current Version | Available Version | Arch\n--+---+---+---+---+---\nv | Main | curl | 8.0.1-1 | 8.6.0-2 | x86_64\n");
        assert_eq!(zyp, vec![PendingUpdate { name: "curl".into(), current: "8.0.1-1".into(), candidate: "8.6.0-2".into() }]);
    }

    #[test]
    fn search_results() {
        let apt = parse_search(
            PackageManager::Apt,
            "htop - interactive processes viewer\nbtop - Modern and colorful command line resource monitor\n",
        );
        assert_eq!((apt[0].name.as_str(), apt[0].description.as_str()), ("htop", "interactive processes viewer"));
        let dnf = parse_search(PackageManager::Dnf, "===== Name Matched: htop =====\nhtop.x86_64 : Interactive process viewer\n");
        assert_eq!(dnf[0].name, "htop");
        let pac = parse_search(
            PackageManager::Pacman,
            "extra/htop 3.3.0-1\n    Interactive process viewer\ncore/vim 9.1-1 [installed]\n    Vi Improved\n",
        );
        assert_eq!(pac.len(), 2);
        assert_eq!((pac[1].name.as_str(), pac[1].description.as_str()), ("vim", "Vi Improved"));
    }

    #[test]
    fn every_manager_has_commands() {
        for (pm, _) in PackageManager::ALL {
            let c = commands(pm);
            assert!(!c.list.is_empty() && !c.refresh.is_empty() && !c.upgrade.is_empty());
        }
    }
}
