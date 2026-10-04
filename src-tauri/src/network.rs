//! Listening sockets, established connections, and server-side network
//! diagnostics (ping, traceroute, DNS, HTTP, MTR, port check).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::JobMeta;
use crate::shell::{q, validate};
use crate::ssh::session::Exec;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Listener {
    pub protocol: String,
    pub address: String,
    pub port: String,
    /// Empty when the owning process is not visible without root.
    pub process: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub state: String,
    pub local: String,
    pub remote: String,
}

/// Split `addr:port` at the last colon (IPv6 addresses contain colons).
fn split_endpoint(endpoint: &str) -> (String, String) {
    match endpoint.rfind(':') {
        Some(i) => (endpoint[..i].to_string(), endpoint[i + 1..].to_string()),
        None => (endpoint.to_string(), String::new()),
    }
}

/// `users:(("sshd",pid=812,fd=3),("sshd",pid=1,fd=4))` → first process name and pid.
fn parse_ss_process(text: &str) -> (String, Option<u32>) {
    let name = text.split('"').nth(1).unwrap_or("").to_string();
    let pid = text.split("pid=").nth(1).and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next()).and_then(|p| p.parse().ok());
    (name, pid)
}

/// Parse `ss -tulpnH`.
pub fn parse_ss_listening(text: &str) -> Vec<Listener> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            // Netid State Recv-Q Send-Q Local Peer [Process]
            if f.len() < 6 || !(f[0].starts_with("tcp") || f[0].starts_with("udp")) {
                return None;
            }
            let (address, port) = split_endpoint(f[4]);
            let (process, pid) = f.get(6).map(|p| parse_ss_process(p)).unwrap_or_default();
            Some(Listener { protocol: f[0].to_string(), address, port, process, pid })
        })
        .collect()
}

/// Parse `netstat -tulpn` (fallback when `ss` is missing).
pub fn parse_netstat_listening(text: &str) -> Vec<Listener> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 4 || !(f[0].starts_with("tcp") || f[0].starts_with("udp")) {
                return None;
            }
            let (address, port) = split_endpoint(f[3]);
            // The last column is "PID/Program name" or "-".
            let (pid, process) = f
                .last()
                .and_then(|p| p.split_once('/'))
                .map(|(pid, name)| (pid.parse().ok(), name.to_string()))
                .unwrap_or((None, String::new()));
            Some(Listener { protocol: f[0].to_string(), address, port, process, pid })
        })
        .collect()
}

/// Parse `ss -tnH` (all TCP sockets with a state column), keeping established ones.
pub fn parse_ss_connections(text: &str) -> Vec<Connection> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            // State Recv-Q Send-Q Local Peer
            (f.len() >= 5 && f[0] == "ESTAB").then(|| Connection {
                state: "established".to_string(),
                local: f[3].to_string(),
                remote: f[4].to_string(),
            })
        })
        .collect()
}

/// Parse `netstat -tn`.
pub fn parse_netstat_connections(text: &str) -> Vec<Connection> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            (f.len() >= 6 && f[0].starts_with("tcp") && f[5] == "ESTABLISHED").then(|| Connection {
                state: "established".to_string(),
                local: f[3].to_string(),
                remote: f[4].to_string(),
            })
        })
        .collect()
}

/// Listening sockets. With `elevated`, root is used so processes of other users are named.
#[tauri::command]
#[specta::specta]
pub async fn network_listening(state: State<'_, AppState>, elevated: bool) -> AppResult<Vec<Listener>> {
    let session = state.session()?;
    let out = session
        .exec(
            Exec::new("if command -v ss >/dev/null 2>&1; then echo ss; ss -tulpnH; else echo netstat; netstat -tulpn 2>/dev/null; fi")
                .sudo_if(elevated),
        )
        .await?
        .into_stdout()?;
    let (tool, body) = out.split_once('\n').unwrap_or((&out, ""));
    Ok(if tool == "ss" { parse_ss_listening(body) } else { parse_netstat_listening(body) })
}

#[tauri::command]
#[specta::specta]
pub async fn network_connections(state: State<'_, AppState>) -> AppResult<Vec<Connection>> {
    let session = state.session()?;
    let out =
        session.run("if command -v ss >/dev/null 2>&1; then echo ss; ss -tnH; else echo netstat; netstat -tn 2>/dev/null; fi").await?;
    let (tool, body) = out.split_once('\n').unwrap_or((&out, ""));
    Ok(if tool == "ss" { parse_ss_connections(body) } else { parse_netstat_connections(body) })
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Interface {
    pub name: String,
    pub addresses: Vec<String>,
}

/// Parse `ip -o addr show`.
pub fn parse_ip_addr(text: &str) -> Vec<Interface> {
    let mut interfaces: Vec<Interface> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        // "2: eth0    inet 10.0.0.5/24 brd … scope global eth0"
        if f.len() < 4 || !(f[2] == "inet" || f[2] == "inet6") {
            continue;
        }
        let name = f[1].trim_end_matches(':').to_string();
        match interfaces.iter_mut().find(|i| i.name == name) {
            Some(i) => i.addresses.push(f[3].to_string()),
            None => interfaces.push(Interface { name, addresses: vec![f[3].to_string()] }),
        }
    }
    interfaces
}

#[tauri::command]
#[specta::specta]
pub async fn network_interfaces(state: State<'_, AppState>) -> AppResult<Vec<Interface>> {
    let session = state.session()?;
    let out = session.exec(Exec::new("ip -o addr show 2>/dev/null")).await?;
    Ok(parse_ip_addr(&out.stdout))
}

// ---------------------------------------------------------------- diagnostics

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "tool", rename_all = "camelCase")]
pub enum Diagnostic {
    Ping,
    Traceroute,
    Dns { record: String },
    Http,
    Mtr,
    Port { port: u32 },
}

const DNS_RECORDS: [&str; 8] = ["A", "AAAA", "MX", "TXT", "NS", "CNAME", "SOA", "PTR"];

/// A URL for the HTTP check: scheme optional, no shell-significant characters needed.
fn http_target(target: &str) -> AppResult<String> {
    let url =
        if target.starts_with("http://") || target.starts_with("https://") { target.to_string() } else { format!("https://{target}") };
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_only = host.rsplit_once(':').map_or(host, |(h, port)| if port.bytes().all(|b| b.is_ascii_digit()) { h } else { host });
    validate::host(host_only.trim_matches(['[', ']']))?;
    if url.bytes().any(|b| b.is_ascii_whitespace() || b.is_ascii_control()) {
        return Err(AppError::invalid("The URL cannot contain spaces"));
    }
    Ok(url)
}

/// Build the shell script for a diagnostic (pure, so it can be tested).
pub fn diagnostic_script(diagnostic: &Diagnostic, target: &str) -> AppResult<String> {
    let target = target.trim();
    if let Diagnostic::Http = diagnostic {
        let url = q(&http_target(target)?);
        return Ok(format!(
            "curl -sS -L -o /dev/null -D - --max-time 20 -w '\\n--- timing ---\\nDNS lookup:    %{{time_namelookup}}s\\nTCP connect:   %{{time_connect}}s\\nTLS handshake: %{{time_appconnect}}s\\nFirst byte:    %{{time_starttransfer}}s\\nTotal:         %{{time_total}}s\\nHTTP status:   %{{http_code}}\\nRemote IP:     %{{remote_ip}}\\n' {url}"
        ));
    }
    let host = q(validate::host(target)?);
    Ok(match diagnostic {
        Diagnostic::Ping => format!("ping -c 5 -W 3 {host}"),
        Diagnostic::Traceroute => {
            format!("if command -v traceroute >/dev/null 2>&1; then traceroute -w 2 {host}; else tracepath {host}; fi")
        }
        Diagnostic::Dns { record } => {
            let record = validate::one_of("record type", record, &DNS_RECORDS)?;
            if record == "PTR" {
                format!(
                    "if command -v dig >/dev/null 2>&1; then dig +noall +answer +comments -x {host}; \
                     elif command -v host >/dev/null 2>&1; then host {host}; else nslookup {host}; fi"
                )
            } else {
                format!(
                    "if command -v dig >/dev/null 2>&1; then dig +noall +answer +comments {host} {record}; \
                     elif command -v host >/dev/null 2>&1; then host -t {record} {host}; else nslookup -type={record} {host}; fi"
                )
            }
        }
        Diagnostic::Mtr => format!("mtr --report --report-wide --report-cycles 10 {host}"),
        Diagnostic::Port { port } => {
            let port = validate::port(*port)?;
            format!("if command -v nc >/dev/null 2>&1; then nc -zv -w 5 {host} {port} 2>&1; else ncat -zv -w 5 {host} {port} 2>&1; fi")
        }
        Diagnostic::Http => unreachable!("handled above"),
    })
}

/// Run a diagnostic from the server as a hidden streamed job.
#[tauri::command]
#[specta::specta]
pub async fn netdiag_run(app: AppHandle, state: State<'_, AppState>, diagnostic: Diagnostic, target: String) -> AppResult<String> {
    let session = state.session()?;
    let script = diagnostic_script(&diagnostic, &target)?;
    // MTR needs raw sockets on most systems.
    let sudo = matches!(diagnostic, Diagnostic::Mtr) && !session.is_root();
    let exec = Exec::new(format!("{script} 2>&1"));
    let exec = if sudo && session.sudo_ready().await { exec.sudo() } else { exec };
    state.jobs.start(&app, session, JobMeta::hidden("Network diagnostic"), exec).await
}

#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IpInfo {
    pub ip: String,
    pub country: String,
    pub country_code: String,
    pub region: String,
    pub city: String,
    pub postal: String,
    pub org: String,
    pub asn: String,
    pub timezone: String,
    pub latitude: String,
    pub longitude: String,
    /// The service the data came from.
    pub provider: String,
}

pub fn parse_ipapi(body: &str) -> AppResult<IpInfo> {
    let v: serde_json::Value = serde_json::from_str(body)?;
    if v.get("error").and_then(|e| e.as_bool()) == Some(true) {
        let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("lookup failed");
        return Err(AppError::new(ErrorCode::Http, reason));
    }
    let text = |key: &str| match v.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    Ok(IpInfo {
        ip: text("ip"),
        country: text("country_name"),
        country_code: text("country_code"),
        region: text("region"),
        city: text("city"),
        postal: text("postal"),
        org: text("org"),
        asn: text("asn"),
        timezone: text("timezone"),
        latitude: text("latitude"),
        longitude: text("longitude"),
        provider: "ipapi.co".to_string(),
    })
}

/// Geolocation of an IP address. This request is made from the desktop
/// (to ipapi.co), not from the server.
#[tauri::command]
#[specta::specta]
pub async fn ip_info(ip: String) -> AppResult<IpInfo> {
    let ip = validate::ip(ip.trim())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("jarvis-server-manager/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client.get(format!("https://ipapi.co/{ip}/json/")).send().await?;
    let status = response.status();
    let body = response.text().await?;
    if status.as_u16() == 429 {
        return Err(AppError::new(ErrorCode::Http, "ipapi.co rate limit reached; try again later"));
    }
    parse_ipapi(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ss_listening() {
        let text = "\
udp   UNCONN 0      0            127.0.0.53%lo:53         0.0.0.0:*    users:((\"systemd-resolve\",pid=612,fd=13))
tcp   LISTEN 0      4096               0.0.0.0:22         0.0.0.0:*    users:((\"sshd\",pid=812,fd=3),(\"systemd\",pid=1,fd=99))
tcp   LISTEN 0      511                   [::]:80            [::]:*
tcp   LISTEN 0      128                  [::1]:5432          [::]:*    users:((\"postgres\",pid=1500,fd=6))
";
        let l = parse_ss_listening(text);
        assert_eq!(l.len(), 4);
        assert_eq!(
            l[0],
            Listener {
                protocol: "udp".into(),
                address: "127.0.0.53%lo".into(),
                port: "53".into(),
                process: "systemd-resolve".into(),
                pid: Some(612)
            }
        );
        assert_eq!((l[1].process.as_str(), l[1].pid), ("sshd", Some(812)));
        // Not visible without root: no process column.
        assert_eq!((l[2].address.as_str(), l[2].port.as_str(), l[2].process.as_str(), l[2].pid), ("[::]", "80", "", None));
        assert_eq!(l[3].address, "[::1]");
    }

    #[test]
    fn netstat_listening() {
        let text = "Active Internet connections (only servers)\nProto Recv-Q Send-Q Local Address           Foreign Address         State       PID/Program name\n\
tcp        0      0 0.0.0.0:22              0.0.0.0:*               LISTEN      812/sshd\n\
tcp6       0      0 :::80                   :::*                    LISTEN      -\n\
udp        0      0 0.0.0.0:68              0.0.0.0:*                           640/dhclient\n";
        let l = parse_netstat_listening(text);
        assert_eq!(l.len(), 3);
        assert_eq!((l[0].port.as_str(), l[0].process.as_str(), l[0].pid), ("22", "sshd", Some(812)));
        assert_eq!((l[1].address.as_str(), l[1].port.as_str(), l[1].pid), ("::", "80", None));
        assert_eq!(l[2].process, "dhclient");
    }

    #[test]
    fn connections() {
        let ss = "ESTAB  0 0 10.0.0.5:22 203.0.113.9:51234\nLISTEN 0 128 0.0.0.0:22 0.0.0.0:*\nESTAB 0 0 [::1]:5432 [::1]:40000\n";
        let c = parse_ss_connections(ss);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].local.as_str(), c[0].remote.as_str()), ("10.0.0.5:22", "203.0.113.9:51234"));
        let ns = "tcp 0 0 10.0.0.5:22 203.0.113.9:51234 ESTABLISHED\ntcp 0 0 0.0.0.0:22 0.0.0.0:* LISTEN\n";
        assert_eq!(parse_netstat_connections(ns).len(), 1);
    }

    #[test]
    fn interfaces() {
        let text = "1: lo    inet 127.0.0.1/8 scope host lo\\       valid_lft forever\n2: eth0    inet 10.0.0.5/24 brd 10.0.0.255 scope global eth0\n2: eth0    inet6 fe80::1/64 scope link\n";
        let i = parse_ip_addr(text);
        assert_eq!(i.len(), 2);
        assert_eq!(i[1].addresses, vec!["10.0.0.5/24", "fe80::1/64"]);
    }

    #[test]
    fn diagnostic_scripts_validate_targets() {
        assert_eq!(diagnostic_script(&Diagnostic::Ping, " example.com ").unwrap(), "ping -c 5 -W 3 example.com");
        assert!(diagnostic_script(&Diagnostic::Ping, "example.com; reboot").is_err());
        assert!(diagnostic_script(&Diagnostic::Ping, "$(id)").is_err());
        assert!(diagnostic_script(&Diagnostic::Dns { record: "MX".into() }, "example.com")
            .unwrap()
            .contains("dig +noall +answer +comments example.com MX"));
        assert!(diagnostic_script(&Diagnostic::Dns { record: "ANY; rm".into() }, "example.com").is_err());
        assert!(diagnostic_script(&Diagnostic::Dns { record: "PTR".into() }, "8.8.8.8").unwrap().contains("-x 8.8.8.8"));
        assert!(diagnostic_script(&Diagnostic::Port { port: 443 }, "example.com").unwrap().contains("nc -zv -w 5 example.com 443"));
        assert!(diagnostic_script(&Diagnostic::Port { port: 0 }, "example.com").is_err());
        assert!(diagnostic_script(&Diagnostic::Mtr, "2001:db8::1").unwrap().ends_with("2001:db8::1"));
    }

    #[test]
    fn http_targets() {
        assert_eq!(http_target("example.com").unwrap(), "https://example.com");
        assert_eq!(http_target("http://example.com:8080/a?b=1&c=2").unwrap(), "http://example.com:8080/a?b=1&c=2");
        assert!(http_target("https://exa mple.com").is_err());
        assert!(http_target("https://;reboot/").is_err());
        let script = diagnostic_script(&Diagnostic::Http, "https://example.com/a?b=1&c=2").unwrap();
        assert!(script.ends_with("'https://example.com/a?b=1&c=2'"));
    }

    #[test]
    fn ipapi_response() {
        let ok = r#"{"ip":"8.8.8.8","city":"Mountain View","region":"California","country_name":"United States","country_code":"US","postal":"94043","latitude":37.42301,"longitude":-122.083352,"timezone":"America/Los_Angeles","asn":"AS15169","org":"GOOGLE"}"#;
        let info = parse_ipapi(ok).unwrap();
        assert_eq!(info.city, "Mountain View");
        assert_eq!(info.latitude, "37.42301");
        assert_eq!(info.asn, "AS15169");
        assert_eq!(info.provider, "ipapi.co");
        let err = parse_ipapi(r#"{"error":true,"reason":"Reserved IP Address"}"#).unwrap_err();
        assert_eq!(err.details.as_deref(), Some("Reserved IP Address"));
    }
}
