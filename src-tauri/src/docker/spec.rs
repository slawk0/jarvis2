//! Building `docker create` command lines from a container specification,
//! and the safe "recreate with new settings" script.

use serde::Deserialize;
use specta::Type;

use super::parse::{Mount, PortMapping};
use crate::error::{AppError, AppResult};
use crate::shell::{q, validate, Cmd};

/// Everything the "edit container" form can change.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContainerSpec {
    pub name: String,
    pub image: String,
    /// Command line, split like a shell would (quotes allowed). Empty keeps the image default.
    pub command: String,
    /// Entrypoint override; empty keeps the image default.
    pub entrypoint: String,
    pub working_dir: String,
    pub user: String,
    pub hostname: String,
    pub tty: bool,
    pub interactive: bool,
    pub restart_policy: String,
    pub env: Vec<(String, String)>,
    pub labels: Vec<(String, String)>,
    pub ports: Vec<PortMapping>,
    pub mounts: Vec<Mount>,
    /// Primary network (name, or `bridge` / `host` / `none`).
    pub network: String,
    pub extra_networks: Vec<String>,
    pub dns: Vec<String>,
    pub log_driver: String,
    pub log_options: Vec<(String, String)>,
    /// Docker size strings such as `512m`; empty means unlimited/default.
    pub memory: String,
    pub memory_reservation: String,
    pub cpus: String,
    pub shm_size: String,
    pub privileged: bool,
    pub cap_add: Vec<String>,
    pub cap_drop: Vec<String>,
}

pub fn restart_policy(policy: &str) -> AppResult<&str> {
    let base = policy.split(':').next().unwrap_or("");
    let retries_ok = policy.split_once(':').is_none_or(|(_, n)| base == "on-failure" && n.parse::<u32>().is_ok());
    if matches!(base, "no" | "always" | "unless-stopped" | "on-failure") && retries_ok {
        Ok(policy)
    } else {
        Err(AppError::invalid("Invalid restart policy"))
    }
}

/// Split a command line into words, honouring single and double quotes and backslashes.
pub fn split_words(line: &str) -> AppResult<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => current.push(ch),
                        None => return Err(AppError::invalid("Unclosed single quote in command")),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(ch @ ('"' | '\\' | '$' | '`')) => current.push(ch),
                            Some(ch) => {
                                current.push('\\');
                                current.push(ch);
                            }
                            None => return Err(AppError::invalid("Unclosed double quote in command")),
                        },
                        Some(ch) => current.push(ch),
                        None => return Err(AppError::invalid("Unclosed double quote in command")),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(ch) = chars.next() {
                    current.push(ch);
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                current.push(c);
            }
        }
    }
    if in_word {
        words.push(current);
    }
    Ok(words)
}

/// Join words back into a command line that `split_words` reads identically.
pub fn join_words(words: &[String]) -> String {
    words.iter().map(|w| q(w)).collect::<Vec<_>>().join(" ")
}

fn size(what: &str, value: &str) -> AppResult<Option<String>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let split = value.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    if number.parse::<f64>().is_ok() && matches!(unit.to_ascii_lowercase().as_str(), "" | "b" | "k" | "m" | "g") {
        Ok(Some(value.to_string()))
    } else {
        Err(AppError::invalid(format!("Invalid {what}: use a number with b, k, m or g")))
    }
}

fn port_number(what: &str, value: &str) -> AppResult<String> {
    validate::port_or_range(value.trim(), '-').map_err(|_| AppError::invalid(format!("Invalid {what}: {value}")))
}

impl ContainerSpec {
    /// The `docker create` command line for this specification.
    pub fn create_command(&self) -> AppResult<String> {
        let name = validate::name("container name", self.name.trim())?;
        let image = validate::name("image", self.image.trim())?;
        let mut cmd = Cmd::new("docker").lit("create").opt("--name", name);

        cmd = cmd.opt_eq("--restart", restart_policy(self.restart_policy.trim())?);
        cmd = cmd.lit_if(self.tty, "-t").lit_if(self.interactive, "-i").lit_if(self.privileged, "--privileged");

        let network = self.network.trim();
        if !network.is_empty() {
            cmd = cmd.opt("--network", validate::name("network", network)?);
        }
        let hostname = self.hostname.trim();
        if !hostname.is_empty() && network != "host" {
            cmd = cmd.opt("--hostname", validate::host(hostname)?);
        }
        if !self.working_dir.trim().is_empty() {
            cmd = cmd.opt("--workdir", validate::abs_path(self.working_dir.trim())?);
        }
        if !self.user.trim().is_empty() {
            cmd = cmd.opt("--user", validate::single_line("user", self.user.trim())?);
        }
        if !self.entrypoint.trim().is_empty() {
            cmd = cmd.opt("--entrypoint", validate::single_line("entrypoint", self.entrypoint.trim())?);
        }

        for (key, value) in &self.env {
            if key.trim().is_empty() {
                continue;
            }
            cmd = cmd.opt("-e", format!("{}={value}", validate::env_key(key.trim())?));
        }
        for (key, value) in &self.labels {
            if key.trim().is_empty() {
                continue;
            }
            validate::single_line("label", key)?;
            cmd = cmd.opt("--label", format!("{}={value}", key.trim()));
        }
        if network != "host" && network != "none" {
            for port in &self.ports {
                if port.container_port.trim().is_empty() {
                    continue;
                }
                let protocol = validate::one_of("protocol", port.protocol.trim(), &["tcp", "udp"])?;
                let container = port_number("container port", &port.container_port)?;
                let mut mapping = String::new();
                if !port.host_ip.trim().is_empty() {
                    let ip = validate::ip(port.host_ip.trim())?;
                    mapping.push_str(&if ip.contains(':') { format!("[{ip}]:") } else { format!("{ip}:") });
                }
                if !port.host_port.trim().is_empty() {
                    mapping.push_str(&port_number("host port", &port.host_port)?);
                    mapping.push(':');
                } else if !mapping.is_empty() {
                    mapping.push(':');
                }
                cmd = cmd.opt("-p", format!("{mapping}{container}/{protocol}"));
            }
        }
        for mount in &self.mounts {
            let target = validate::abs_path(mount.target.trim())?;
            match mount.kind.as_str() {
                "tmpfs" => cmd = cmd.opt("--tmpfs", target),
                kind => {
                    let source = mount.source.trim();
                    if kind == "bind" {
                        validate::abs_path(source)?;
                    } else {
                        validate::name("volume", source)?;
                    }
                    if source.contains(':') || target.contains(':') {
                        return Err(AppError::invalid("Mount paths cannot contain “:”"));
                    }
                    let suffix = if mount.read_only { ":ro" } else { "" };
                    cmd = cmd.opt("-v", format!("{source}:{target}{suffix}"));
                }
            }
        }
        for server in &self.dns {
            if !server.trim().is_empty() {
                cmd = cmd.opt("--dns", validate::ip(server.trim())?);
            }
        }
        if !self.log_driver.trim().is_empty() {
            cmd = cmd.opt("--log-driver", validate::slug("log driver", self.log_driver.trim())?);
            for (key, value) in &self.log_options {
                if !key.trim().is_empty() {
                    cmd = cmd.opt("--log-opt", format!("{}={value}", validate::slug("log option", key.trim())?));
                }
            }
        }
        cmd = cmd
            .opt_some("--memory", size("memory limit", &self.memory)?)
            .opt_some("--memory-reservation", size("memory reservation", &self.memory_reservation)?)
            .opt_some("--shm-size", size("shm size", &self.shm_size)?);
        let cpus = self.cpus.trim();
        if !cpus.is_empty() {
            if cpus.parse::<f64>().map_or(true, |n| n <= 0.0) {
                return Err(AppError::invalid("CPUs must be a positive number"));
            }
            cmd = cmd.opt("--cpus", cpus);
        }
        for cap in &self.cap_add {
            cmd = cmd.opt("--cap-add", validate::slug("capability", cap.trim())?);
        }
        for cap in &self.cap_drop {
            cmd = cmd.opt("--cap-drop", validate::slug("capability", cap.trim())?);
        }

        cmd = cmd.arg(image).args(split_words(&self.command)?);
        Ok(cmd.build())
    }
}

/// Shell script that replaces `current` with a container built from `spec`:
/// rename old → stop old → create and start new → remove old. On any failure
/// the new container is removed and the old one is renamed back and restarted.
pub fn recreate_script(current: &str, spec: &ContainerSpec, stamp: i64) -> AppResult<String> {
    let create = spec.create_command()?;
    let old = q(validate::name("container", current)?);
    let new = q(spec.name.trim());
    let backup = q(&format!("{}-jarvis-old-{stamp}", current.trim_start_matches('/')));
    let mut connects = String::new();
    for network in &spec.extra_networks {
        let network = network.trim();
        if !network.is_empty() && network != spec.network.trim() {
            connects.push_str(&format!(" && docker network connect {} {new}", q(validate::name("network", network)?)));
        }
    }
    Ok(format!(
        "was_running=$(docker inspect --format '{{{{.State.Running}}}}' {old}) || exit 1\n\
         echo 'Renaming the current container…'\n\
         docker rename {old} {backup} || exit 1\n\
         restore() {{\n\
           echo 'Failed: restoring the previous container.' >&2\n\
           docker rm -f {new} >/dev/null 2>&1\n\
           docker rename {backup} {old}\n\
           [ \"$was_running\" = true ] && docker start {old} >/dev/null\n\
           exit 1\n\
         }}\n\
         echo 'Stopping it…'\n\
         docker stop {backup} >/dev/null || restore\n\
         echo 'Creating the new container…'\n\
         {{ {create} >/dev/null{connects} && docker start {new} >/dev/null; }} || restore\n\
         echo 'Removing the previous container…'\n\
         docker rm {backup} >/dev/null\n\
         echo 'Done.'"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ContainerSpec {
        ContainerSpec {
            name: "web".into(),
            image: "nginx:1.27".into(),
            command: String::new(),
            entrypoint: String::new(),
            working_dir: String::new(),
            user: String::new(),
            hostname: String::new(),
            tty: false,
            interactive: false,
            restart_policy: "no".into(),
            env: vec![],
            labels: vec![],
            ports: vec![],
            mounts: vec![],
            network: String::new(),
            extra_networks: vec![],
            dns: vec![],
            log_driver: String::new(),
            log_options: vec![],
            memory: String::new(),
            memory_reservation: String::new(),
            cpus: String::new(),
            shm_size: String::new(),
            privileged: false,
            cap_add: vec![],
            cap_drop: vec![],
        }
    }

    #[test]
    fn minimal_command() {
        assert_eq!(base().create_command().unwrap(), "docker create --name web --restart=no nginx:1.27");
    }

    #[test]
    fn full_command_quotes_everything() {
        let mut s = base();
        s.command = "nginx -g 'daemon off;'".into();
        s.restart_policy = "unless-stopped".into();
        s.network = "appnet".into();
        s.hostname = "web".into();
        s.env = vec![("MSG".into(), "hello world; rm -rf /".into()), ("EMPTY".into(), String::new())];
        s.labels = vec![("traefik.enable".into(), "true".into())];
        s.ports = vec![
            PortMapping { host_ip: String::new(), host_port: "8080".into(), container_port: "80".into(), protocol: "tcp".into() },
            PortMapping { host_ip: "127.0.0.1".into(), host_port: "5353".into(), container_port: "53".into(), protocol: "udp".into() },
        ];
        s.mounts = vec![
            Mount { kind: "bind".into(), source: "/srv/my site".into(), target: "/usr/share/nginx/html".into(), read_only: true },
            Mount { kind: "volume".into(), source: "cache".into(), target: "/cache".into(), read_only: false },
        ];
        s.memory = "512m".into();
        s.cpus = "1.5".into();
        s.cap_add = vec!["NET_ADMIN".into()];
        s.dns = vec!["1.1.1.1".into()];
        s.log_driver = "json-file".into();
        s.log_options = vec![("max-size".into(), "10m".into())];
        s.tty = true;
        assert_eq!(
            s.create_command().unwrap(),
            "docker create --name web --restart=unless-stopped -t --network appnet --hostname web \
             -e 'MSG=hello world; rm -rf /' -e EMPTY= --label traefik.enable=true \
             -p 8080:80/tcp -p 127.0.0.1:5353:53/udp \
             -v '/srv/my site:/usr/share/nginx/html:ro' -v cache:/cache --dns 1.1.1.1 \
             --log-driver json-file --log-opt max-size=10m --memory 512m --cpus 1.5 --cap-add NET_ADMIN \
             nginx:1.27 nginx -g 'daemon off;'"
        );
    }

    #[test]
    fn rejects_dangerous_or_invalid_values() {
        let mut s = base();
        s.name = "web; reboot".into();
        assert!(s.create_command().is_err());
        let mut s = base();
        s.image = "$(evil)".into();
        assert!(s.create_command().is_err());
        let mut s = base();
        s.restart_policy = "sometimes".into();
        assert!(s.create_command().is_err());
        let mut s = base();
        s.memory = "lots".into();
        assert!(s.create_command().is_err());
        let mut s = base();
        s.ports =
            vec![PortMapping { host_ip: String::new(), host_port: "99999".into(), container_port: "80".into(), protocol: "tcp".into() }];
        assert!(s.create_command().is_err());
        let mut s = base();
        s.mounts = vec![Mount { kind: "bind".into(), source: "relative".into(), target: "/x".into(), read_only: false }];
        assert!(s.create_command().is_err());
        let mut s = base();
        s.command = "echo 'unclosed".into();
        assert!(s.create_command().is_err());
        assert!(restart_policy("on-failure:5").is_ok());
        assert!(restart_policy("always:5").is_err());
    }

    #[test]
    fn host_network_drops_ports_and_hostname() {
        let mut s = base();
        s.network = "host".into();
        s.hostname = "x".into();
        s.ports = vec![PortMapping { host_ip: String::new(), host_port: "80".into(), container_port: "80".into(), protocol: "tcp".into() }];
        assert_eq!(s.create_command().unwrap(), "docker create --name web --restart=no --network host nginx:1.27");
    }

    #[test]
    fn word_splitting_round_trips() {
        assert_eq!(split_words("nginx -g 'daemon off;'").unwrap(), vec!["nginx", "-g", "daemon off;"]);
        assert_eq!(split_words(r#"sh -c "echo \"hi\" $HOME""#).unwrap(), vec!["sh", "-c", "echo \"hi\" $HOME"]);
        assert_eq!(split_words("  a\\ b   ''  ").unwrap(), vec!["a b", ""]);
        assert!(split_words("").unwrap().is_empty());
        let original = vec!["sh".to_string(), "-c".to_string(), "echo 'it''s' \"x\"; ls".to_string(), String::new()];
        assert_eq!(split_words(&join_words(&original)).unwrap(), original);
    }

    #[test]
    fn recreate_script_restores_on_failure() {
        let mut s = base();
        s.extra_networks = vec!["backend".into()];
        let script = recreate_script("web", &s, 1700000000).unwrap();
        assert!(script.contains("docker rename web web-jarvis-old-1700000000 || exit 1"));
        assert!(script.contains("docker stop web-jarvis-old-1700000000 >/dev/null || restore"));
        assert!(script.contains("docker create --name web --restart=no nginx:1.27 >/dev/null && docker network connect backend web && docker start web >/dev/null; } || restore"));
        assert!(script.contains("docker rename web-jarvis-old-1700000000 web"));
        assert!(script.trim_end().ends_with("echo 'Done.'"));
        assert!(recreate_script("bad name", &s, 1).is_err());
    }
}
