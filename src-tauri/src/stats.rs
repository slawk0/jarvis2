//! System statistics for the Dashboard and the process list.
//!
//! Everything is read from `/proc`, POSIX `df -P` and `ps -o`, so it works on
//! GNU and BusyBox userlands alike. Counters are returned raw; the frontend
//! turns two samples into rates.
//!
//! Float fields carry `#[specta(type = i32)]`: specta would otherwise export
//! them as `number | null`, and these values are always finite.

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BasicStats {
    /// Jiffies since boot, all CPUs: total and idle (idle + iowait).
    pub cpu_total: u64,
    pub cpu_idle: u64,
    pub cpu_count: u32,
    pub mem_total: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    #[specta(type = [i32; 3])]
    pub load: [f64; 3],
    #[specta(type = i32)]
    pub uptime_secs: f64,
    /// Interface carrying the default route (empty if none was found).
    pub net_interface: String,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub root_total: u64,
    pub root_used: u64,
}

const BASIC_SCRIPT: &str = "head -n 1 /proc/stat; grep -c '^cpu[0-9]' /proc/stat; echo '#mem'; cat /proc/meminfo; \
     echo '#load'; cat /proc/loadavg; echo '#uptime'; cat /proc/uptime; \
     echo '#route'; cat /proc/net/route 2>/dev/null; echo '#net'; cat /proc/net/dev; \
     echo '#df'; df -Pk / 2>/dev/null | tail -n 1";

pub fn parse_basic(text: &str) -> AppResult<BasicStats> {
    let mut stats = BasicStats::default();
    let mut section = "cpu";
    let mut route_iface: Option<String> = None;
    let mut interfaces: Vec<(String, u64, u64)> = Vec::new();
    let mut cpu_seen = false;

    for line in text.lines() {
        if let Some(name) = line.strip_prefix('#') {
            section = match name {
                "mem" => "mem",
                "load" => "load",
                "uptime" => "uptime",
                "route" => "route",
                "net" => "net",
                "df" => "df",
                _ => section,
            };
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match section {
            "cpu" => {
                if fields.first() == Some(&"cpu") {
                    let values: Vec<u64> = fields[1..].iter().filter_map(|v| v.parse().ok()).collect();
                    if values.len() >= 4 {
                        // user nice system idle iowait irq softirq steal (guest values are
                        // already included in user/nice).
                        stats.cpu_total = values.iter().take(8).sum();
                        stats.cpu_idle = values[3] + values.get(4).copied().unwrap_or(0);
                        cpu_seen = true;
                    }
                } else if let Some(count) = fields.first().and_then(|v| v.parse().ok()) {
                    stats.cpu_count = count;
                }
            }
            "mem" => {
                let kb = fields.get(1).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024;
                match fields.first().copied() {
                    Some("MemTotal:") => stats.mem_total = kb,
                    Some("MemAvailable:") => stats.mem_available = kb,
                    // Kernels before 3.14 have no MemAvailable; MemFree is a lower bound.
                    Some("MemFree:") if stats.mem_available == 0 => stats.mem_available = kb,
                    Some("SwapTotal:") => stats.swap_total = kb,
                    Some("SwapFree:") => stats.swap_free = kb,
                    _ => {}
                }
            }
            "load" => {
                for (slot, value) in stats.load.iter_mut().zip(&fields) {
                    *slot = value.parse().unwrap_or(0.0);
                }
            }
            "uptime" => {
                stats.uptime_secs = fields.first().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            }
            "route" => {
                // Iface Destination Gateway Flags … ; destination 00000000 is the default route.
                if fields.len() > 1 && fields[1] == "00000000" && route_iface.is_none() {
                    route_iface = Some(fields[0].to_string());
                }
            }
            "net" => {
                if let Some((name, rest)) = line.split_once(':') {
                    let values: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    if values.len() >= 9 {
                        interfaces.push((name.trim().to_string(), values[0], values[8]));
                    }
                }
            }
            // Filesystem 1024-blocks Used Available Capacity Mounted-on
            "df" if fields.len() >= 6 => {
                stats.root_total = fields[1].parse::<u64>().unwrap_or(0) * 1024;
                stats.root_used = fields[2].parse::<u64>().unwrap_or(0) * 1024;
            }
            _ => {}
        }
    }

    if !cpu_seen || stats.mem_total == 0 {
        return Err(AppError::parse("unexpected /proc output"));
    }
    let chosen = route_iface.and_then(|name| interfaces.iter().find(|(n, _, _)| *n == name).cloned()).or_else(|| {
        // No default route: fall back to the busiest real interface.
        interfaces.iter().filter(|(n, _, _)| n != "lo").max_by_key(|(_, rx, tx)| rx + tx).cloned()
    });
    if let Some((name, rx, tx)) = chosen {
        stats.net_interface = name;
        stats.net_rx_bytes = rx;
        stats.net_tx_bytes = tx;
    }
    Ok(stats)
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Partition {
    pub device: String,
    pub fs_type: String,
    pub mount: String,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    #[specta(type = i32)]
    pub use_percent: f64,
    /// `None` when the filesystem does not report inodes (e.g. btrfs).
    #[specta(type = Option<i32>)]
    pub inode_percent: Option<f64>,
}

const PSEUDO_FS: [&str; 14] = [
    "tmpfs",
    "devtmpfs",
    "squashfs",
    "overlay",
    "proc",
    "sysfs",
    "cgroup",
    "cgroup2",
    "devpts",
    "efivarfs",
    "ramfs",
    "fuse.snapfuse",
    "nsfs",
    "autofs",
];

pub fn is_pseudo_fs(fs_type: &str, device: &str) -> bool {
    PSEUDO_FS.contains(&fs_type) || device.starts_with("/dev/loop")
}

/// Parse `df -PTk` (sizes) joined with `df -Pi` (inodes).
pub fn parse_partitions(sizes: &str, inodes: &str) -> Vec<Partition> {
    let inode_use: Vec<(String, Option<f64>)> = inodes
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            // Filesystem Inodes IUsed IFree IUse% Mounted-on
            if f.len() < 6 {
                return None;
            }
            let total: f64 = f[1].parse().ok()?;
            let used: f64 = f[2].parse().unwrap_or(0.0);
            // Network and virtual filesystems report nonsense (even negative) counts.
            let percent = (total > 0.0 && (0.0..=total).contains(&used)).then(|| used / total * 100.0);
            Some((f[5..].join(" "), percent))
        })
        .collect();

    sizes
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            // Filesystem Type 1024-blocks Used Available Capacity Mounted-on
            if f.len() < 7 {
                return None;
            }
            let total = f[2].parse::<u64>().ok()? * 1024;
            let used = f[3].parse::<u64>().ok()? * 1024;
            let available = f[4].parse::<u64>().ok()? * 1024;
            let mount = f[6..].join(" ");
            let denominator = used + available;
            Some(Partition {
                device: f[0].to_string(),
                fs_type: f[1].to_string(),
                use_percent: if denominator > 0 { used as f64 / denominator as f64 * 100.0 } else { 0.0 },
                inode_percent: inode_use.iter().find(|(m, _)| *m == mount).and_then(|(_, p)| *p),
                mount,
                total,
                used,
                available,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Process {
    pub pid: u32,
    pub user: String,
    #[specta(type = i32)]
    pub cpu: f64,
    #[specta(type = i32)]
    pub mem: f64,
    pub nice: Option<i32>,
    pub command: String,
}

const PS_SCRIPT: &str = "ps -eo pid,user,pcpu,pmem,ni,args 2>/dev/null || ps -o pid,user,vsz,args";

/// Parse `ps -eo pid,user,pcpu,pmem,ni,args`. BusyBox `ps` has no CPU/MEM
/// columns; its fallback layout yields zeros for those.
pub fn parse_processes(text: &str) -> Vec<Process> {
    let mut lines = text.lines();
    let header = lines.next().unwrap_or("");
    let full = header.contains("%CPU");
    lines
        .filter_map(|line| {
            let line = line.trim_start();
            let mut rest = line;
            let mut next = || {
                let (word, tail) = rest.split_once(char::is_whitespace)?;
                rest = tail.trim_start();
                Some(word)
            };
            let pid = next()?.parse().ok()?;
            let user = next()?.to_string();
            if full {
                let cpu = next()?.parse().unwrap_or(0.0);
                let mem = next()?.parse().unwrap_or(0.0);
                let nice = next()?.parse().ok();
                Some(Process { pid, user, cpu, mem, nice, command: rest.to_string() })
            } else {
                next()?; // vsz
                Some(Process { pid, user, cpu: 0.0, mem: 0.0, nice: None, command: rest.to_string() })
            }
        })
        .filter(|p| !p.command.is_empty())
        .collect()
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os: String,
    pub hostname: String,
    pub kernel: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExtendedStats {
    pub system: SystemInfo,
    pub partitions: Vec<Partition>,
    /// Top processes by memory.
    pub top_processes: Vec<Process>,
}

pub fn parse_os_release(text: &str) -> String {
    let field = |key: &str| {
        text.lines().find_map(|l| l.strip_prefix(key)).map(|v| v.trim().trim_matches('"').to_string()).filter(|v| !v.is_empty())
    };
    field("PRETTY_NAME=").or_else(|| field("NAME=")).unwrap_or_else(|| "Linux".to_string())
}

pub async fn basic(session: &Session) -> AppResult<BasicStats> {
    let out = session.exec(Exec::new(BASIC_SCRIPT).secs(15)).await?;
    parse_basic(&out.stdout)
}

pub async fn processes(session: &Session) -> AppResult<Vec<Process>> {
    Ok(parse_processes(&session.run(PS_SCRIPT).await?))
}

pub async fn extended(session: &Session) -> AppResult<ExtendedStats> {
    const MARK: &str = "#jarvis-section";
    let script = format!(
        "cat /etc/os-release 2>/dev/null; echo '{MARK}'; \
         cat /proc/sys/kernel/hostname 2>/dev/null || hostname; echo '{MARK}'; uname -r; echo '{MARK}'; \
         df -PTk 2>/dev/null; echo '{MARK}'; df -Pi 2>/dev/null; echo '{MARK}'; {PS_SCRIPT}"
    );
    let out = session.exec(Exec::new(script).secs(20)).await?.stdout;
    let parts: Vec<&str> = out.split(MARK).collect();
    let part = |i: usize| parts.get(i).copied().unwrap_or("").trim_matches('\n');
    let mut top = parse_processes(part(5));
    top.sort_by(|a, b| b.mem.total_cmp(&a.mem));
    top.truncate(8);
    Ok(ExtendedStats {
        system: SystemInfo { os: parse_os_release(part(0)), hostname: part(1).trim().to_string(), kernel: part(2).trim().to_string() },
        partitions: parse_partitions(part(3), part(4))
            .into_iter()
            .filter(|p| p.total > 0 && (p.mount == "/" || !is_pseudo_fs(&p.fs_type, &p.device)))
            .collect(),
        top_processes: top,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn stats_basic(state: State<'_, AppState>) -> AppResult<BasicStats> {
    let session = state.session()?;
    basic(&session).await
}

#[tauri::command]
#[specta::specta]
pub async fn stats_extended(state: State<'_, AppState>) -> AppResult<ExtendedStats> {
    let session = state.session()?;
    extended(&session).await
}

#[tauri::command]
#[specta::specta]
pub async fn process_list(state: State<'_, AppState>) -> AppResult<Vec<Process>> {
    let session = state.session()?;
    processes(&session).await
}

#[derive(Debug, Clone, Copy, serde::Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProcessSignal {
    Term,
    Kill,
}

/// Signal a process; elevates automatically when it belongs to someone else.
#[tauri::command]
#[specta::specta]
pub async fn process_signal(state: State<'_, AppState>, pid: u32, signal: ProcessSignal) -> AppResult<()> {
    let session = state.session()?;
    let name = match signal {
        ProcessSignal::Term => "TERM",
        ProcessSignal::Kill => "KILL",
    };
    session.run_auto(format!("kill -{name} {pid}")).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn process_renice(state: State<'_, AppState>, pid: u32, nice: i32) -> AppResult<()> {
    if !(-20..=19).contains(&nice) {
        return Err(AppError::invalid("Nice value must be between -20 and 19"));
    }
    let session = state.session()?;
    session.run_auto(format!("renice -n {nice} -p {pid}")).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC: &str = "cpu  10 2 5 100 3 1 1 0 0 0
4
#mem
MemTotal:        8000000 kB
MemFree:         1000000 kB
MemAvailable:    6000000 kB
SwapTotal:       2000000 kB
SwapFree:        1500000 kB
#load
0.52 0.40 0.31 2/345 1234
#uptime
93784.12 300000.00
#route
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
ens3\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
ens3\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
#net
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 9999999   100    0    0    0     0          0         0  9999999   100    0    0    0     0       0          0
  ens3: 5000   50    0    0    0     0          0         0  7000   40    0    0    0     0       0          0
docker0: 1 1 0 0 0 0 0 0 2 1 0 0 0 0 0 0
#df
/dev/vda1       41152736 20576368  18463140      53% /
";

    #[test]
    fn parses_basic_stats() {
        let s = parse_basic(BASIC).unwrap();
        assert_eq!(s.cpu_total, 122);
        assert_eq!(s.cpu_idle, 103);
        assert_eq!(s.cpu_count, 4);
        assert_eq!(s.mem_total, 8_000_000 * 1024);
        assert_eq!(s.mem_available, 6_000_000 * 1024);
        assert_eq!(s.swap_free, 1_500_000 * 1024);
        assert_eq!(s.load, [0.52, 0.40, 0.31]);
        assert!((s.uptime_secs - 93784.12).abs() < 1e-9);
        // The default-route interface wins over the busier loopback.
        assert_eq!(s.net_interface, "ens3");
        assert_eq!((s.net_rx_bytes, s.net_tx_bytes), (5000, 7000));
        assert_eq!(s.root_total, 41_152_736 * 1024);
        assert_eq!(s.root_used, 20_576_368 * 1024);
    }

    #[test]
    fn basic_without_default_route_or_memavailable() {
        let text = BASIC.replace("ens3\t00000000\t0101A8C0", "ens3\t0002A8C0\t0101A8C0").replace("MemAvailable:    6000000 kB\n", "");
        let s = parse_basic(&text).unwrap();
        assert_eq!(s.net_interface, "ens3");
        assert_eq!(s.mem_available, 1_000_000 * 1024);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_basic("bash: command not found").is_err());
    }

    #[test]
    fn parses_partitions_with_inodes() {
        let sizes = "Filesystem     Type     1024-blocks     Used Available Capacity Mounted on
/dev/vda1      ext4        41152736 20576368  18463140      53% /
tmpfs          tmpfs         401000     1200    399800       1% /run
/dev/vdb1      xfs        103081248 92773123  10308125      90% /mnt/data disk
/dev/loop0     squashfs       56832    56832         0     100% /snap/core/1
";
        let inodes = "Filesystem      Inodes  IUsed   IFree IUse% Mounted on
/dev/vda1      2621440 262144 2359296   10% /
tmpfs           100250    900   99350    1% /run
/dev/vdb1            0      0       0     - /mnt/data disk
/dev/loop0         999 -99999  100998     - /snap/core/1
";
        let parts = parse_partitions(sizes, inodes);
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0].mount, "/");
        assert_eq!(parts[0].fs_type, "ext4");
        assert!((parts[0].use_percent - 52.7).abs() < 0.1);
        assert!((parts[0].inode_percent.unwrap() - 10.0).abs() < 1e-9);
        assert_eq!(parts[2].mount, "/mnt/data disk");
        assert_eq!(parts[2].inode_percent, None);
        assert_eq!(parts[3].inode_percent, None);
        assert!(is_pseudo_fs(&parts[1].fs_type, &parts[1].device));
        assert!(is_pseudo_fs(&parts[3].fs_type, &parts[3].device));
        assert!(!is_pseudo_fs(&parts[0].fs_type, &parts[0].device));
    }

    #[test]
    fn parses_gnu_ps() {
        let text = "    PID USER     %CPU %MEM  NI COMMAND
      1 root      0.0  0.1   0 /sbin/init splash
    812 www-data 12.5  3.4  -5 nginx: worker process
   9001 deploy    0.3 10.0  19 node /srv/app/server.js --port 3000
   9002 root      0.0  0.0   - [kworker/0:1]
";
        let p = parse_processes(text);
        assert_eq!(p.len(), 4);
        assert_eq!(p[1].pid, 812);
        assert_eq!(p[1].user, "www-data");
        assert_eq!(p[1].cpu, 12.5);
        assert_eq!(p[1].nice, Some(-5));
        assert_eq!(p[1].command, "nginx: worker process");
        assert_eq!(p[2].command, "node /srv/app/server.js --port 3000");
        assert_eq!(p[3].nice, None);
    }

    #[test]
    fn parses_busybox_ps_fallback() {
        let text = "PID   USER     VSZ  COMMAND
    1 root     1620 /sbin/init
   42 nobody   800 crond -f
";
        let p = parse_processes(text);
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].command, "crond -f");
        assert_eq!(p[1].cpu, 0.0);
    }

    #[test]
    fn os_release_name() {
        assert_eq!(parse_os_release("NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n"), "Ubuntu 24.04.1 LTS");
        assert_eq!(parse_os_release("NAME=Alpine\n"), "Alpine");
        assert_eq!(parse_os_release(""), "Linux");
    }
}
