//! Parsers for `docker … --format '{{json .}}'` and `docker inspect` output.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::error::{AppError, AppResult};

fn json_lines<T: for<'de> Deserialize<'de>>(text: &str) -> Vec<T> {
    text.lines().filter_map(|line| serde_json::from_str::<T>(line.trim()).ok()).collect()
}

fn labels_map(labels: &str) -> BTreeMap<String, String> {
    labels.split(',').filter_map(|pair| pair.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

// ---------------------------------------------------------------- containers

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `paused`, `created`, `restarting`, `dead`.
    pub state: String,
    /// Human status, e.g. "Up 3 hours (healthy)".
    pub status: String,
    pub ports: String,
    pub created: String,
    /// Compose project this container belongs to, if any.
    pub compose_project: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PsLine {
    #[serde(rename = "ID")]
    id: String,
    names: String,
    image: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    ports: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    labels: String,
}

pub fn parse_containers(text: &str) -> Vec<Container> {
    json_lines::<PsLine>(text)
        .into_iter()
        .map(|c| {
            // Docker older than 20.10 has no State field; derive it from Status.
            let state = if c.state.is_empty() {
                let s = c.status.to_lowercase();
                if s.contains("paused") {
                    "paused"
                } else if s.starts_with("up") {
                    "running"
                } else if s.starts_with("created") {
                    "created"
                } else if s.starts_with("restarting") {
                    "restarting"
                } else {
                    "exited"
                }
                .to_string()
            } else {
                c.state
            };
            Container {
                compose_project: labels_map(&c.labels).remove("com.docker.compose.project").unwrap_or_default(),
                id: c.id,
                name: c.names.split(',').next().unwrap_or("").to_string(),
                image: c.image,
                state,
                status: c.status,
                ports: c.ports,
                created: c.created_at,
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortMapping {
    pub host_ip: String,
    pub host_port: String,
    pub container_port: String,
    /// `tcp` or `udp`.
    pub protocol: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Mount {
    /// `bind`, `volume` or `tmpfs`.
    pub kind: String,
    /// Host path or volume name.
    pub source: String,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAttachment {
    pub name: String,
    pub ip: String,
    pub gateway: String,
    pub mac: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContainerDetail {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub created: String,
    pub started: String,
    pub command: Vec<String>,
    /// `command` as one shell-quoted line, as the edit form shows it.
    pub command_line: String,
    pub entrypoint: Vec<String>,
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
    pub networks: Vec<NetworkAttachment>,
    pub network_mode: String,
    pub dns: Vec<String>,
    pub log_driver: String,
    pub log_options: Vec<(String, String)>,
    /// Bytes; 0 means unlimited.
    pub memory: u64,
    pub memory_reservation: u64,
    /// CPU limit in units of 1e-9 CPUs; 0 means unlimited.
    pub nano_cpus: u64,
    pub shm_size: u64,
    pub privileged: bool,
    pub cap_add: Vec<String>,
    pub cap_drop: Vec<String>,
    /// Non-empty when the container is managed by Docker Compose.
    pub compose_project: String,
}

fn text(v: &Value, pointer: &str) -> String {
    v.pointer(pointer).and_then(Value::as_str).unwrap_or("").to_string()
}

fn strings(v: &Value, pointer: &str) -> Vec<String> {
    v.pointer(pointer)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn pairs(v: &Value, pointer: &str) -> Vec<(String, String)> {
    v.pointer(pointer)
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect())
        .unwrap_or_default()
}

fn number(v: &Value, pointer: &str) -> u64 {
    v.pointer(pointer).and_then(Value::as_u64).unwrap_or(0)
}

fn flag(v: &Value, pointer: &str) -> bool {
    v.pointer(pointer).and_then(Value::as_bool).unwrap_or(false)
}

pub fn parse_container_detail(json: &str) -> AppResult<ContainerDetail> {
    let value: Value = serde_json::from_str(json)?;
    let c = value.get(0).ok_or_else(|| AppError::parse("docker inspect returned nothing"))?;

    let mut ports = Vec::new();
    if let Some(bindings) = c.pointer("/HostConfig/PortBindings").and_then(Value::as_object) {
        for (key, hosts) in bindings {
            let (container_port, protocol) = key.split_once('/').unwrap_or((key, "tcp"));
            for host in hosts.as_array().into_iter().flatten() {
                ports.push(PortMapping {
                    host_ip: text(host, "/HostIp"),
                    host_port: text(host, "/HostPort"),
                    container_port: container_port.to_string(),
                    protocol: protocol.to_string(),
                });
            }
        }
    }

    let mounts = c
        .pointer("/Mounts")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|m| {
                    let kind = text(m, "/Type");
                    Mount {
                        source: if kind == "volume" { text(m, "/Name") } else { text(m, "/Source") },
                        kind,
                        target: text(m, "/Destination"),
                        read_only: !m.pointer("/RW").and_then(Value::as_bool).unwrap_or(true),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let networks = c
        .pointer("/NetworkSettings/Networks")
        .and_then(Value::as_object)
        .map(|nets| {
            nets.iter()
                .map(|(name, n)| NetworkAttachment {
                    name: name.clone(),
                    ip: text(n, "/IPAddress"),
                    gateway: text(n, "/Gateway"),
                    mac: text(n, "/MacAddress"),
                })
                .collect()
        })
        .unwrap_or_default();

    let env = strings(c, "/Config/Env")
        .into_iter()
        .map(|e| match e.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (e, String::new()),
        })
        .collect();

    let labels = pairs(c, "/Config/Labels");
    let compose_project = labels.iter().find(|(k, _)| k == "com.docker.compose.project").map(|(_, v)| v.clone()).unwrap_or_default();

    let policy = text(c, "/HostConfig/RestartPolicy/Name");
    Ok(ContainerDetail {
        id: text(c, "/Id"),
        name: text(c, "/Name").trim_start_matches('/').to_string(),
        image: text(c, "/Config/Image"),
        state: text(c, "/State/Status"),
        created: text(c, "/Created"),
        started: text(c, "/State/StartedAt"),
        command_line: super::spec::join_words(&strings(c, "/Config/Cmd")),
        command: strings(c, "/Config/Cmd"),
        entrypoint: strings(c, "/Config/Entrypoint"),
        working_dir: text(c, "/Config/WorkingDir"),
        user: text(c, "/Config/User"),
        hostname: text(c, "/Config/Hostname"),
        tty: flag(c, "/Config/Tty"),
        interactive: flag(c, "/Config/OpenStdin"),
        restart_policy: if policy.is_empty() { "no".into() } else { policy },
        env,
        labels,
        ports,
        mounts,
        networks,
        network_mode: text(c, "/HostConfig/NetworkMode"),
        dns: strings(c, "/HostConfig/Dns"),
        log_driver: text(c, "/HostConfig/LogConfig/Type"),
        log_options: pairs(c, "/HostConfig/LogConfig/Config"),
        memory: number(c, "/HostConfig/Memory"),
        memory_reservation: number(c, "/HostConfig/MemoryReservation"),
        nano_cpus: number(c, "/HostConfig/NanoCpus"),
        shm_size: number(c, "/HostConfig/ShmSize"),
        privileged: flag(c, "/HostConfig/Privileged"),
        cap_add: strings(c, "/HostConfig/CapAdd"),
        cap_drop: strings(c, "/HostConfig/CapDrop"),
        compose_project,
    })
}

// ---------------------------------------------------------------- images

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub size: String,
    pub created: String,
    /// No container (running or stopped) uses this image.
    pub unused: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageLine {
    #[serde(rename = "ID")]
    id: String,
    #[serde(default)]
    repository: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    created_since: String,
}

/// `used` holds one image id (`sha256:…`) per container.
pub fn parse_images(text: &str, used: &str) -> Vec<Image> {
    let used: HashSet<&str> = used.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    json_lines::<ImageLine>(text)
        .into_iter()
        .map(|i| Image {
            unused: !used.contains(i.id.as_str()),
            id: i.id,
            repository: i.repository,
            tag: i.tag,
            size: i.size,
            created: i.created_since,
        })
        .collect()
}

// ---------------------------------------------------------------- networks & volumes

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
}

pub fn parse_networks(text: &str) -> Vec<Network> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Line {
        #[serde(rename = "ID")]
        id: String,
        name: String,
        #[serde(default)]
        driver: String,
        #[serde(default)]
        scope: String,
    }
    json_lines::<Line>(text).into_iter().map(|n| Network { id: n.id, name: n.name, driver: n.driver, scope: n.scope }).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub unused: bool,
}

pub fn parse_volumes(text: &str, used: &str) -> Vec<Volume> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Line {
        name: String,
        #[serde(default)]
        driver: String,
        #[serde(default)]
        mountpoint: String,
    }
    let used: HashSet<&str> = used.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    json_lines::<Line>(text)
        .into_iter()
        .map(|v| Volume {
            unused: !used.contains(v.name.as_str()),
            // Some versions omit the mount point from `volume ls`; the default driver's layout is fixed.
            mountpoint: if v.mountpoint.is_empty() && v.driver == "local" {
                format!("/var/lib/docker/volumes/{}/_data", v.name)
            } else {
                v.mountpoint
            },
            name: v.name,
            driver: v.driver,
        })
        .collect()
}

// ---------------------------------------------------------------- stats

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStats {
    pub id: String,
    pub name: String,
    #[specta(type = i32)]
    pub cpu_percent: f64,
    #[specta(type = i32)]
    pub mem_percent: f64,
    pub mem_used: u64,
    pub mem_limit: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub block_read: u64,
    pub block_write: u64,
    pub pids: u32,
}

/// "12.5MiB", "1.9GiB", "3.4kB", "0B" → bytes.
pub fn parse_size(text: &str) -> u64 {
    let text = text.trim();
    let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let value: f64 = text[..split].parse().unwrap_or(0.0);
    let factor = match text[split..].trim() {
        "B" | "" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KiB" => 1024.0,
        "MiB" => 1024f64.powi(2),
        "GiB" => 1024f64.powi(3),
        "TiB" => 1024f64.powi(4),
        _ => 1.0,
    };
    (value * factor) as u64
}

fn size_pair(text: &str) -> (u64, u64) {
    let (a, b) = text.split_once('/').unwrap_or((text, ""));
    (parse_size(a), parse_size(b))
}

pub fn parse_stats(text: &str) -> Vec<ContainerStats> {
    #[derive(Deserialize)]
    struct Line {
        #[serde(rename = "ID", default)]
        id: String,
        #[serde(rename = "Container", default)]
        container: String,
        #[serde(rename = "Name", default)]
        name: String,
        #[serde(rename = "CPUPerc", default)]
        cpu: String,
        #[serde(rename = "MemPerc", default)]
        mem: String,
        #[serde(rename = "MemUsage", default)]
        mem_usage: String,
        #[serde(rename = "NetIO", default)]
        net: String,
        #[serde(rename = "BlockIO", default)]
        block: String,
        #[serde(rename = "PIDs", default)]
        pids: String,
    }
    let percent = |s: &str| s.trim().trim_end_matches('%').parse::<f64>().unwrap_or(0.0);
    json_lines::<Line>(text)
        .into_iter()
        .map(|s| {
            let (mem_used, mem_limit) = size_pair(&s.mem_usage);
            let (net_rx, net_tx) = size_pair(&s.net);
            let (block_read, block_write) = size_pair(&s.block);
            ContainerStats {
                id: if s.id.is_empty() { s.container } else { s.id },
                name: s.name,
                cpu_percent: percent(&s.cpu),
                mem_percent: percent(&s.mem),
                mem_used,
                mem_limit,
                net_rx,
                net_tx,
                block_read,
                block_write,
                pids: s.pids.trim().parse().unwrap_or(0),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containers_from_ps_json() {
        let text = r#"{"Command":"\"nginx -g 'daemon off;'\"","CreatedAt":"2026-09-30 10:00:00 +0000 UTC","ID":"abc123","Image":"nginx:1.27","Labels":"com.docker.compose.project=blog,com.docker.compose.service=web,maintainer=NGINX","Names":"blog-web-1","Ports":"0.0.0.0:8080->80/tcp, :::8080->80/tcp","State":"running","Status":"Up 3 hours (healthy)"}
{"CreatedAt":"2026-09-01","ID":"def456","Image":"redis","Labels":"","Names":"cache","Ports":"","Status":"Exited (0) 2 days ago"}
not json"#;
        let c = parse_containers(text);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].name, "blog-web-1");
        assert_eq!(c[0].state, "running");
        assert_eq!(c[0].compose_project, "blog");
        assert_eq!(c[0].ports, "0.0.0.0:8080->80/tcp, :::8080->80/tcp");
        // No State field (old Docker): derived from Status.
        assert_eq!(c[1].state, "exited");
        assert_eq!(c[1].compose_project, "");
    }

    const INSPECT: &str = r#"[{
      "Id": "abc123", "Created": "2026-09-30T10:00:00Z", "Name": "/web",
      "State": {"Status": "running", "StartedAt": "2026-10-01T08:00:00Z"},
      "HostConfig": {
        "NetworkMode": "appnet", "Privileged": false, "Memory": 536870912, "MemoryReservation": 0, "NanoCpus": 1500000000, "ShmSize": 67108864,
        "RestartPolicy": {"Name": "unless-stopped", "MaximumRetryCount": 0},
        "PortBindings": {"80/tcp": [{"HostIp": "", "HostPort": "8080"}], "53/udp": [{"HostIp": "127.0.0.1", "HostPort": "5353"}]},
        "Dns": ["1.1.1.1"], "CapAdd": ["NET_ADMIN"], "CapDrop": null,
        "LogConfig": {"Type": "json-file", "Config": {"max-size": "10m"}}
      },
      "Mounts": [
        {"Type": "bind", "Source": "/srv/www", "Destination": "/usr/share/nginx/html", "RW": false},
        {"Type": "volume", "Name": "web_cache", "Source": "/var/lib/docker/volumes/web_cache/_data", "Destination": "/cache", "RW": true}
      ],
      "Config": {
        "Hostname": "web", "User": "", "Tty": false, "OpenStdin": false, "Image": "nginx:1.27", "WorkingDir": "/app",
        "Env": ["PATH=/usr/bin", "EMPTY=", "URL=http://x/?a=b"], "Cmd": ["nginx", "-g", "daemon off;"], "Entrypoint": ["/docker-entrypoint.sh"],
        "Labels": {"com.docker.compose.project": "blog", "a": "b"}
      },
      "NetworkSettings": {"Networks": {"appnet": {"IPAddress": "172.20.0.5", "Gateway": "172.20.0.1", "MacAddress": "02:42:ac:14:00:05"}}}
    }]"#;

    #[test]
    fn container_detail_from_inspect() {
        let d = parse_container_detail(INSPECT).unwrap();
        assert_eq!(d.name, "web");
        assert_eq!(d.state, "running");
        assert_eq!(d.command, vec!["nginx", "-g", "daemon off;"]);
        assert_eq!(d.entrypoint, vec!["/docker-entrypoint.sh"]);
        assert_eq!(d.restart_policy, "unless-stopped");
        assert_eq!(d.env[2], ("URL".to_string(), "http://x/?a=b".to_string()));
        assert_eq!(d.env[1], ("EMPTY".to_string(), String::new()));
        assert_eq!(d.ports.len(), 2);
        let udp = d.ports.iter().find(|p| p.protocol == "udp").unwrap();
        assert_eq!((udp.host_ip.as_str(), udp.host_port.as_str(), udp.container_port.as_str()), ("127.0.0.1", "5353", "53"));
        assert_eq!(
            d.mounts[0],
            Mount { kind: "bind".into(), source: "/srv/www".into(), target: "/usr/share/nginx/html".into(), read_only: true }
        );
        assert_eq!(d.mounts[1].source, "web_cache");
        assert_eq!(d.networks[0].ip, "172.20.0.5");
        assert_eq!(d.memory, 536_870_912);
        assert_eq!(d.nano_cpus, 1_500_000_000);
        assert_eq!(d.cap_add, vec!["NET_ADMIN"]);
        assert!(d.cap_drop.is_empty());
        assert_eq!(d.log_options, vec![("max-size".to_string(), "10m".to_string())]);
        assert_eq!(d.compose_project, "blog");
        assert!(parse_container_detail("[]").is_err());
    }

    #[test]
    fn images_networks_volumes() {
        let images = parse_images(
            "{\"ID\":\"sha256:aaa\",\"Repository\":\"nginx\",\"Tag\":\"1.27\",\"Size\":\"188MB\",\"CreatedSince\":\"3 weeks ago\"}\n{\"ID\":\"sha256:bbb\",\"Repository\":\"<none>\",\"Tag\":\"<none>\",\"Size\":\"5MB\",\"CreatedSince\":\"1 day ago\"}\n",
            "sha256:aaa\nsha256:aaa\n",
        );
        assert!(!images[0].unused);
        assert!(images[1].unused);
        assert_eq!(images[0].size, "188MB");

        let networks = parse_networks("{\"ID\":\"n1\",\"Name\":\"bridge\",\"Driver\":\"bridge\",\"Scope\":\"local\"}\n");
        assert_eq!(networks[0].name, "bridge");

        let volumes = parse_volumes(
            "{\"Name\":\"db_data\",\"Driver\":\"local\",\"Mountpoint\":\"\"}\n{\"Name\":\"nfs\",\"Driver\":\"nfs\",\"Mountpoint\":\"/mnt/x\"}\n",
            "db_data\n\n",
        );
        assert_eq!(volumes[0].mountpoint, "/var/lib/docker/volumes/db_data/_data");
        assert!(!volumes[0].unused);
        assert!(volumes[1].unused);
    }

    #[test]
    fn stats_and_sizes() {
        assert_eq!(parse_size("0B"), 0);
        assert_eq!(parse_size("3.4kB"), 3400);
        assert_eq!(parse_size("12.5MiB"), 13_107_200);
        assert_eq!(parse_size("1.5GiB"), 1_610_612_736);
        assert_eq!(parse_size(" 2GB "), 2_000_000_000);
        let s = parse_stats(
            "{\"BlockIO\":\"1.2MB / 0B\",\"CPUPerc\":\"12.34%\",\"Container\":\"abc\",\"ID\":\"abc\",\"MemPerc\":\"1.50%\",\"MemUsage\":\"30MiB / 2GiB\",\"Name\":\"web\",\"NetIO\":\"1kB / 2kB\",\"PIDs\":\"7\"}\n",
        );
        assert_eq!(s[0].name, "web");
        assert_eq!(s[0].cpu_percent, 12.34);
        assert_eq!(s[0].mem_used, 30 * 1024 * 1024);
        assert_eq!(s[0].mem_limit, 2 * 1024 * 1024 * 1024);
        assert_eq!((s[0].net_rx, s[0].net_tx), (1000, 2000));
        assert_eq!(s[0].block_read, 1_200_000);
        assert_eq!(s[0].pids, 7);
    }
}
