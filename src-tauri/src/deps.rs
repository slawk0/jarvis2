//! Registry of server-side tools: how to detect each one and how to install
//! it with whatever package manager the server has.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::JobMeta;
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    Systemd,
    Cron,
    Docker,
    Mtr,
    Traceroute,
    Netcat,
    Dig,
    Curl,
    Ping,
    Restic,
    Rclone,
    Nginx,
    Certbot,
    CertbotNginx,
    CertbotDnsCloudflare,
    CertbotDnsDigitalocean,
    CertbotDnsRoute53,
    Ufw,
    Iptables,
    Crowdsec,
    CrowdsecFirewallBouncer,
    MysqlClient,
    PostgresClient,
    Parted,
    Growpart,
    Lsblk,
    Ss,
    Zip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PackageManager {
    Apt,
    Dnf,
    Yum,
    Apk,
    Pacman,
    Zypper,
}

impl PackageManager {
    pub const ALL: [(PackageManager, &'static str); 6] = [
        (PackageManager::Apt, "apt-get"),
        (PackageManager::Dnf, "dnf"),
        (PackageManager::Yum, "yum"),
        (PackageManager::Apk, "apk"),
        (PackageManager::Pacman, "pacman"),
        (PackageManager::Zypper, "zypper"),
    ];

    pub fn from_binary(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, bin)| *bin == name.trim())
            .map(|(pm, _)| *pm)
    }

    /// Shell snippet printing the first package manager binary found.
    pub fn detect_script() -> &'static str {
        "for pm in apt-get dnf yum apk pacman zypper; do \
         if command -v $pm >/dev/null 2>&1; then echo $pm; break; fi; done"
    }

    /// Non-interactive install command for a set of (trusted, static) package names.
    pub fn install_command(self, packages: &[&str]) -> String {
        let pkgs = packages.join(" ");
        match self {
            PackageManager::Apt => format!(
                "export DEBIAN_FRONTEND=noninteractive; apt-get update -q && apt-get install -y -q {pkgs}"
            ),
            PackageManager::Dnf => format!("dnf install -y {pkgs}"),
            PackageManager::Yum => format!("yum install -y {pkgs}"),
            PackageManager::Apk => format!("apk add --no-cache {pkgs}"),
            PackageManager::Pacman => format!("pacman -Sy --noconfirm --needed {pkgs}"),
            PackageManager::Zypper => format!("zypper --non-interactive install {pkgs}"),
        }
    }
}

/// Package names per manager, in the order apt, dnf/yum, apk, pacman, zypper.
#[derive(Debug, Clone, Copy)]
struct Packages([&'static [&'static str]; 5]);

impl Packages {
    const fn same(names: &'static [&'static str]) -> Self {
        Self([names, names, names, names, names])
    }

    fn for_manager(&self, pm: PackageManager) -> &'static [&'static str] {
        match pm {
            PackageManager::Apt => self.0[0],
            PackageManager::Dnf | PackageManager::Yum => self.0[1],
            PackageManager::Apk => self.0[2],
            PackageManager::Pacman => self.0[3],
            PackageManager::Zypper => self.0[4],
        }
    }
}

struct Spec {
    label: &'static str,
    /// Shell test that succeeds when the tool is usable.
    probe: &'static str,
    packages: Option<Packages>,
    /// Runs instead of the package install (as root). `{pkg_install}` expands
    /// to the package-manager command for `packages`, if any.
    installer: Option<&'static str>,
    /// Runs after a successful install (as root); failures are ignored.
    post_install: Option<&'static str>,
    docs: &'static str,
    installable: bool,
}

const START_CRON: &str = "systemctl enable --now cron 2>/dev/null || systemctl enable --now crond 2>/dev/null || \
     { rc-update add crond default 2>/dev/null; rc-service crond start 2>/dev/null; } || true";

const INSTALL_DOCKER: &str = "if command -v apk >/dev/null 2>&1; then \
       apk add --no-cache docker docker-cli-compose && rc-update add docker default && rc-service docker start; \
     else \
       f=$(mktemp) && \
       { if command -v curl >/dev/null 2>&1; then curl -fsSL https://get.docker.com -o \"$f\"; \
         else wget -qO \"$f\" https://get.docker.com; fi; } && \
       sh \"$f\"; rc=$?; rm -f \"$f\"; [ $rc -eq 0 ]; \
     fi";

const INSTALL_CROWDSEC_REPO: &str = "if command -v apk >/dev/null 2>&1 || command -v pacman >/dev/null 2>&1; then :; else \
       f=$(mktemp) && \
       { if command -v curl >/dev/null 2>&1; then curl -fsSL https://install.crowdsec.net -o \"$f\"; \
         else wget -qO \"$f\" https://install.crowdsec.net; fi; } && \
       sh \"$f\"; rc=$?; rm -f \"$f\"; [ $rc -eq 0 ]; \
     fi && {pkg_install}";

fn python_module_probe(module: &'static str) -> &'static str {
    match module {
        "certbot_nginx" => {
            "python3 -c 'import certbot_nginx' 2>/dev/null || \
             { command -v snap >/dev/null 2>&1 && snap list certbot >/dev/null 2>&1; }"
        }
        "certbot_dns_cloudflare" => "python3 -c 'import certbot_dns_cloudflare' 2>/dev/null || \
             { command -v snap >/dev/null 2>&1 && snap list certbot-dns-cloudflare >/dev/null 2>&1; }",
        "certbot_dns_digitalocean" => "python3 -c 'import certbot_dns_digitalocean' 2>/dev/null || \
             { command -v snap >/dev/null 2>&1 && snap list certbot-dns-digitalocean >/dev/null 2>&1; }",
        _ => "python3 -c 'import certbot_dns_route53' 2>/dev/null || \
             { command -v snap >/dev/null 2>&1 && snap list certbot-dns-route53 >/dev/null 2>&1; }",
    }
}

fn spec(tool: Tool) -> Spec {
    let simple = |label, probe, packages, docs| Spec {
        label,
        probe,
        packages: Some(packages),
        installer: None,
        post_install: None,
        docs,
        installable: true,
    };
    match tool {
        Tool::Systemd => Spec {
            label: "systemd",
            probe: "[ -d /run/systemd/system ]",
            packages: None,
            installer: None,
            post_install: None,
            docs: "https://systemd.io/",
            installable: false,
        },
        Tool::Cron => Spec {
            post_install: Some(START_CRON),
            ..simple(
                "cron",
                "command -v crontab >/dev/null 2>&1",
                Packages([&["cron"], &["cronie"], &["cronie"], &["cronie"], &["cronie"]]),
                "https://man7.org/linux/man-pages/man5/crontab.5.html",
            )
        },
        Tool::Docker => Spec {
            label: "Docker",
            probe: "command -v docker >/dev/null 2>&1",
            packages: None,
            installer: Some(INSTALL_DOCKER),
            post_install: Some("systemctl enable --now docker 2>/dev/null || true"),
            docs: "https://docs.docker.com/engine/install/",
            installable: true,
        },
        Tool::Mtr => simple(
            "mtr",
            "command -v mtr >/dev/null 2>&1",
            Packages([&["mtr-tiny"], &["mtr"], &["mtr"], &["mtr"], &["mtr"]]),
            "https://www.bitwizard.nl/mtr/",
        ),
        Tool::Traceroute => simple(
            "traceroute",
            "command -v traceroute >/dev/null 2>&1 || command -v tracepath >/dev/null 2>&1",
            Packages([
                &["traceroute"],
                &["traceroute"],
                &["iputils-tracepath"],
                &["traceroute"],
                &["traceroute"],
            ]),
            "https://man7.org/linux/man-pages/man8/traceroute.8.html",
        ),
        Tool::Netcat => simple(
            "netcat",
            "command -v nc >/dev/null 2>&1 || command -v ncat >/dev/null 2>&1",
            Packages([
                &["netcat-openbsd"],
                &["nmap-ncat"],
                &["netcat-openbsd"],
                &["openbsd-netcat"],
                &["netcat-openbsd"],
            ]),
            "https://man.openbsd.org/nc.1",
        ),
        Tool::Dig => simple(
            "dig",
            "command -v dig >/dev/null 2>&1",
            Packages([
                &["dnsutils"],
                &["bind-utils"],
                &["bind-tools"],
                &["bind"],
                &["bind-utils"],
            ]),
            "https://bind9.readthedocs.io/en/latest/manpages.html#dig-dns-lookup-utility",
        ),
        Tool::Curl => simple(
            "curl",
            "command -v curl >/dev/null 2>&1",
            Packages::same(&["curl"]),
            "https://curl.se/docs/",
        ),
        Tool::Ping => simple(
            "ping",
            "command -v ping >/dev/null 2>&1",
            Packages([
                &["iputils-ping"],
                &["iputils"],
                &["iputils"],
                &["iputils"],
                &["iputils"],
            ]),
            "https://man7.org/linux/man-pages/man8/ping.8.html",
        ),
        Tool::Restic => simple(
            "restic",
            "command -v restic >/dev/null 2>&1",
            Packages::same(&["restic"]),
            "https://restic.readthedocs.io/en/stable/020_installation.html",
        ),
        Tool::Rclone => simple(
            "rclone",
            "command -v rclone >/dev/null 2>&1",
            Packages::same(&["rclone"]),
            "https://rclone.org/install/",
        ),
        Tool::Nginx => simple(
            "nginx",
            "command -v nginx >/dev/null 2>&1",
            Packages::same(&["nginx"]),
            "https://nginx.org/en/docs/install.html",
        ),
        Tool::Certbot => simple(
            "certbot",
            "command -v certbot >/dev/null 2>&1",
            Packages::same(&["certbot"]),
            "https://certbot.eff.org/instructions",
        ),
        Tool::CertbotNginx => simple(
            "certbot nginx plugin",
            python_module_probe("certbot_nginx"),
            Packages([
                &["python3-certbot-nginx"],
                &["python3-certbot-nginx"],
                &["certbot-nginx"],
                &["certbot-nginx"],
                &["python3-certbot-nginx"],
            ]),
            "https://eff-certbot.readthedocs.io/en/stable/using.html#nginx",
        ),
        Tool::CertbotDnsCloudflare => simple(
            "certbot Cloudflare DNS plugin",
            python_module_probe("certbot_dns_cloudflare"),
            Packages([
                &["python3-certbot-dns-cloudflare"],
                &["python3-certbot-dns-cloudflare"],
                &["certbot-dns-cloudflare"],
                &["certbot-dns-cloudflare"],
                &["python3-certbot-dns-cloudflare"],
            ]),
            "https://certbot-dns-cloudflare.readthedocs.io/",
        ),
        Tool::CertbotDnsDigitalocean => simple(
            "certbot DigitalOcean DNS plugin",
            python_module_probe("certbot_dns_digitalocean"),
            Packages([
                &["python3-certbot-dns-digitalocean"],
                &["python3-certbot-dns-digitalocean"],
                &["certbot-dns-digitalocean"],
                &["certbot-dns-digitalocean"],
                &["python3-certbot-dns-digitalocean"],
            ]),
            "https://certbot-dns-digitalocean.readthedocs.io/",
        ),
        Tool::CertbotDnsRoute53 => simple(
            "certbot Route 53 DNS plugin",
            python_module_probe("certbot_dns_route53"),
            Packages([
                &["python3-certbot-dns-route53"],
                &["python3-certbot-dns-route53"],
                &["certbot-dns-route53"],
                &["certbot-dns-route53"],
                &["python3-certbot-dns-route53"],
            ]),
            "https://certbot-dns-route53.readthedocs.io/",
        ),
        Tool::Ufw => simple(
            "ufw",
            "command -v ufw >/dev/null 2>&1",
            Packages::same(&["ufw"]),
            "https://help.ubuntu.com/community/UFW",
        ),
        Tool::Iptables => simple(
            "iptables",
            "command -v iptables >/dev/null 2>&1",
            Packages::same(&["iptables"]),
            "https://netfilter.org/projects/iptables/",
        ),
        Tool::Crowdsec => Spec {
            installer: Some(INSTALL_CROWDSEC_REPO),
            post_install: Some("systemctl enable --now crowdsec 2>/dev/null || true"),
            ..simple(
                "CrowdSec",
                "command -v cscli >/dev/null 2>&1",
                Packages::same(&["crowdsec"]),
                "https://docs.crowdsec.net/docs/getting_started/install_crowdsec/",
            )
        },
        Tool::CrowdsecFirewallBouncer => Spec {
            installer: Some(INSTALL_CROWDSEC_REPO),
            post_install: Some(
                "systemctl enable --now crowdsec-firewall-bouncer 2>/dev/null || true",
            ),
            ..simple(
                "CrowdSec firewall bouncer",
                "command -v crowdsec-firewall-bouncer >/dev/null 2>&1 || \
                 [ -e /etc/crowdsec/bouncers/crowdsec-firewall-bouncer.yaml ]",
                Packages([
                    &["crowdsec-firewall-bouncer-iptables"],
                    &["crowdsec-firewall-bouncer-iptables"],
                    &["cs-firewall-bouncer"],
                    &["cs-firewall-bouncer"],
                    &["crowdsec-firewall-bouncer-iptables"],
                ]),
                "https://docs.crowdsec.net/u/bouncers/firewall/",
            )
        },
        Tool::MysqlClient => simple(
            "MySQL client (mysqldump)",
            "command -v mysqldump >/dev/null 2>&1 || command -v mariadb-dump >/dev/null 2>&1",
            Packages([
                &["default-mysql-client"],
                &["mariadb"],
                &["mariadb-client"],
                &["mariadb-clients"],
                &["mariadb-client"],
            ]),
            "https://mariadb.com/kb/en/mariadb-dump/",
        ),
        Tool::PostgresClient => simple(
            "PostgreSQL client (pg_dump)",
            "command -v pg_dump >/dev/null 2>&1",
            Packages([
                &["postgresql-client"],
                &["postgresql"],
                &["postgresql-client"],
                &["postgresql"],
                &["postgresql"],
            ]),
            "https://www.postgresql.org/docs/current/app-pgdump.html",
        ),
        Tool::Parted => simple(
            "parted",
            "command -v parted >/dev/null 2>&1",
            Packages::same(&["parted"]),
            "https://www.gnu.org/software/parted/manual/",
        ),
        Tool::Growpart => simple(
            "growpart",
            "command -v growpart >/dev/null 2>&1",
            Packages([
                &["cloud-guest-utils"],
                &["cloud-utils-growpart"],
                &["cloud-utils-growpart"],
                &["cloud-guest-utils"],
                &["growpart"],
            ]),
            "https://manpages.debian.org/growpart",
        ),
        Tool::Lsblk => simple(
            "lsblk",
            "command -v lsblk >/dev/null 2>&1",
            Packages([
                &["util-linux"],
                &["util-linux"],
                &["lsblk"],
                &["util-linux"],
                &["util-linux"],
            ]),
            "https://man7.org/linux/man-pages/man8/lsblk.8.html",
        ),
        Tool::Ss => simple(
            "ss (iproute2)",
            "command -v ss >/dev/null 2>&1",
            Packages([
                &["iproute2"],
                &["iproute"],
                &["iproute2"],
                &["iproute2"],
                &["iproute2"],
            ]),
            "https://man7.org/linux/man-pages/man8/ss.8.html",
        ),
        Tool::Zip => simple(
            "zip / unzip",
            "command -v zip >/dev/null 2>&1 && command -v unzip >/dev/null 2>&1",
            Packages::same(&["zip", "unzip"]),
            "https://infozip.sourceforge.net/",
        ),
    }
}

/// Root command that installs a tool with the given package manager.
fn install_script(tool: Tool, pm: Option<PackageManager>) -> Option<String> {
    let spec = spec(tool);
    if !spec.installable {
        return None;
    }
    let pkg_install = match (pm, spec.packages) {
        (Some(pm), Some(packages)) => Some(pm.install_command(packages.for_manager(pm))),
        _ => None,
    };
    let mut script = match (spec.installer, pkg_install) {
        (Some(installer), pkg) if installer.contains("{pkg_install}") => {
            installer.replace("{pkg_install}", &pkg?)
        }
        (Some(installer), _) => installer.to_string(),
        (None, Some(pkg)) => pkg,
        (None, None) => return None,
    };
    if let Some(post) = spec.post_install {
        script = format!("{{ {script}; }} && {{ {post}; }}");
    }
    Some(script)
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub tool: Tool,
    pub label: String,
    pub installed: bool,
    /// False for things Jarvis cannot install (e.g. systemd).
    pub installable: bool,
    /// Command the user can run by hand; `None` without a known package manager.
    pub manual_command: Option<String>,
    pub docs_url: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DepsReport {
    pub package_manager: Option<PackageManager>,
    pub tools: Vec<ToolStatus>,
}

pub async fn package_manager(session: &Session) -> AppResult<Option<PackageManager>> {
    let out = session.run(PackageManager::detect_script()).await?;
    Ok(PackageManager::from_binary(&out))
}

/// Probe several tools in one round trip.
pub async fn check(session: &Session, tools: &[Tool]) -> AppResult<DepsReport> {
    let mut script = format!("echo \"pm:$({})\"\n", PackageManager::detect_script());
    for (i, tool) in tools.iter().enumerate() {
        script.push_str(&format!(
            "if {{ {}; }}; then echo \"{i}:1\"; else echo \"{i}:0\"; fi\n",
            spec(*tool).probe
        ));
    }
    let out = session.exec(Exec::new(script).secs(45)).await?.stdout;
    let mut pm = None;
    let mut installed = vec![false; tools.len()];
    for line in out.lines() {
        match line.split_once(':') {
            Some(("pm", name)) => pm = PackageManager::from_binary(name),
            Some((index, flag)) => {
                if let Ok(i) = index.parse::<usize>() {
                    if let Some(slot) = installed.get_mut(i) {
                        *slot = flag.trim() == "1";
                    }
                }
            }
            None => {}
        }
    }
    let tools = tools
        .iter()
        .zip(installed)
        .map(|(tool, installed)| {
            let spec = spec(*tool);
            ToolStatus {
                tool: *tool,
                label: spec.label.to_string(),
                installed,
                installable: spec.installable,
                manual_command: install_script(*tool, pm),
                docs_url: spec.docs.to_string(),
            }
        })
        .collect();
    Ok(DepsReport {
        package_manager: pm,
        tools,
    })
}

/// Fail with `DEPENDENCY_MISSING` unless the tool is present.
pub async fn require(session: &Session, tool: Tool) -> AppResult<()> {
    let probe = spec(tool).probe;
    let out = session.exec(Exec::new(format!("{{ {probe}; }}"))).await?;
    if out.success() {
        Ok(())
    } else {
        Err(AppError::new(ErrorCode::DependencyMissing, spec(tool).label))
    }
}

#[tauri::command]
#[specta::specta]
pub async fn deps_check(state: State<'_, AppState>, tools: Vec<Tool>) -> AppResult<DepsReport> {
    let session = state.session()?;
    check(&session, &tools).await
}

/// Install a tool as a streamed job; returns the job id.
#[tauri::command]
#[specta::specta]
pub async fn deps_install(app: AppHandle, state: State<'_, AppState>, tool: Tool) -> AppResult<String> {
    let session = state.session()?;
    let pm = package_manager(&session).await?;
    let script = install_script(tool, pm).ok_or_else(|| {
        AppError::unsupported(format!(
            "{} cannot be installed automatically on this server",
            spec(tool).label
        ))
    })?;
    let meta = JobMeta::visible(format!("Install {}", spec(tool).label), script.clone());
    state
        .jobs
        .start(&app, session, meta, Exec::new(script).sudo())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Tool; 28] = [
        Tool::Systemd,
        Tool::Cron,
        Tool::Docker,
        Tool::Mtr,
        Tool::Traceroute,
        Tool::Netcat,
        Tool::Dig,
        Tool::Curl,
        Tool::Ping,
        Tool::Restic,
        Tool::Rclone,
        Tool::Nginx,
        Tool::Certbot,
        Tool::CertbotNginx,
        Tool::CertbotDnsCloudflare,
        Tool::CertbotDnsDigitalocean,
        Tool::CertbotDnsRoute53,
        Tool::Ufw,
        Tool::Iptables,
        Tool::Crowdsec,
        Tool::CrowdsecFirewallBouncer,
        Tool::MysqlClient,
        Tool::PostgresClient,
        Tool::Parted,
        Tool::Growpart,
        Tool::Lsblk,
        Tool::Ss,
        Tool::Zip,
    ];

    #[test]
    fn every_tool_has_a_probe_and_docs() {
        for tool in ALL {
            let s = spec(tool);
            assert!(!s.probe.is_empty(), "{tool:?}");
            assert!(s.docs.starts_with("https://"), "{tool:?}");
            assert!(!s.label.is_empty(), "{tool:?}");
        }
    }

    #[test]
    fn every_installable_tool_installs_with_every_manager() {
        for tool in ALL {
            for (pm, _) in PackageManager::ALL {
                let script = install_script(tool, Some(pm));
                assert_eq!(script.is_some(), spec(tool).installable, "{tool:?} {pm:?}");
                if let Some(script) = script {
                    assert!(!script.contains("{pkg_install}"), "{tool:?} {pm:?}");
                }
            }
        }
    }

    #[test]
    fn package_names_differ_per_manager() {
        assert_eq!(
            install_script(Tool::Dig, Some(PackageManager::Apt)).unwrap(),
            "export DEBIAN_FRONTEND=noninteractive; apt-get update -q && apt-get install -y -q dnsutils"
        );
        assert_eq!(
            install_script(Tool::Dig, Some(PackageManager::Dnf)).unwrap(),
            "dnf install -y bind-utils"
        );
        assert_eq!(
            install_script(Tool::Dig, Some(PackageManager::Yum)).unwrap(),
            "yum install -y bind-utils"
        );
        assert_eq!(
            install_script(Tool::Dig, Some(PackageManager::Apk)).unwrap(),
            "apk add --no-cache bind-tools"
        );
        assert_eq!(
            install_script(Tool::Zip, Some(PackageManager::Pacman)).unwrap(),
            "pacman -Sy --noconfirm --needed zip unzip"
        );
        assert_eq!(
            install_script(Tool::Curl, Some(PackageManager::Zypper)).unwrap(),
            "zypper --non-interactive install curl"
        );
    }

    #[test]
    fn systemd_is_not_installable_and_docker_needs_no_manager() {
        assert!(install_script(Tool::Systemd, Some(PackageManager::Apt)).is_none());
        let docker = install_script(Tool::Docker, None).unwrap();
        assert!(docker.contains("get.docker.com"));
        assert!(docker.contains("systemctl enable --now docker"));
        // A plain package cannot be installed without a package manager.
        assert!(install_script(Tool::Curl, None).is_none());
        assert!(install_script(Tool::Crowdsec, None).is_none());
    }

    #[test]
    fn post_install_runs_only_after_success() {
        let cron = install_script(Tool::Cron, Some(PackageManager::Apt)).unwrap();
        assert!(cron.starts_with("{ export DEBIAN_FRONTEND"));
        assert!(cron.contains("; } && { systemctl enable --now cron"));
    }

    #[test]
    fn detects_manager_from_binary_name() {
        assert_eq!(
            PackageManager::from_binary("apt-get\n"),
            Some(PackageManager::Apt)
        );
        assert_eq!(PackageManager::from_binary("apk"), Some(PackageManager::Apk));
        assert_eq!(PackageManager::from_binary(""), None);
    }
}
