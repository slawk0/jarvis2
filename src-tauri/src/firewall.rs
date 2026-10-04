//! Firewall management: UFW and iptables, through typed, validated commands.
//! nftables and firewalld are detected and reported as not supported.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::shell::{q, validate, Cmd};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FirewallBackends {
    pub ufw: bool,
    pub iptables: bool,
    /// Present but not managed by Jarvis.
    pub nftables: bool,
    pub firewalld: bool,
    /// The SSH port of this connection (for lock-out warnings).
    pub ssh_port: u16,
}

#[tauri::command]
#[specta::specta]
pub async fn firewall_detect(state: State<'_, AppState>) -> AppResult<FirewallBackends> {
    let session = state.session()?;
    let out = session
        .run(
            "for t in ufw iptables nft; do command -v $t >/dev/null 2>&1 && echo 1 || echo 0; done; \
             systemctl is-active firewalld >/dev/null 2>&1 && echo 1 || echo 0",
        )
        .await?;
    let flags: Vec<bool> = out.lines().map(|l| l.trim() == "1").collect();
    let flag = |i: usize| flags.get(i).copied().unwrap_or(false);
    Ok(FirewallBackends { ufw: flag(0), iptables: flag(1), nftables: flag(2), firewalld: flag(3), ssh_port: session.target().port })
}

async fn root(session: &Session, script: String) -> AppResult<String> {
    session.exec(Exec::new(script).sudo().secs(60)).await?.into_stdout()
}

// ---------------------------------------------------------------- UFW

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UfwRule {
    pub number: u32,
    pub to: String,
    pub action: String,
    pub from: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UfwStatus {
    pub active: bool,
    pub rules: Vec<UfwRule>,
}

/// Parse `ufw status numbered`.
pub fn parse_ufw_status(text: &str) -> UfwStatus {
    let active = text.lines().any(|l| l.trim().eq_ignore_ascii_case("Status: active"));
    let rules = text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix('[')?;
            let (number, rest) = rest.split_once(']')?;
            let number = number.trim().parse().ok()?;
            // Columns are separated by two or more spaces.
            let columns: Vec<&str> = rest.split("  ").map(str::trim).filter(|c| !c.is_empty()).collect();
            if columns.len() < 3 {
                return None;
            }
            Some(UfwRule { number, to: columns[0].to_string(), action: columns[1].to_string(), from: columns[2..].join(" ") })
        })
        .collect();
    UfwStatus { active, rules }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UfwRuleSpec {
    /// `allow`, `deny` or `reject`.
    pub action: String,
    /// Port or range (`80`, `8000:8100`); empty for "any port".
    pub port: String,
    /// `any`, `tcp` or `udp`.
    pub protocol: String,
    /// Source IP or CIDR; empty for anywhere.
    pub source: String,
}

/// The `ufw …` command line for a new rule.
pub fn ufw_rule_command(spec: &UfwRuleSpec) -> AppResult<String> {
    let action = validate::one_of("action", &spec.action, &["allow", "deny", "reject"])?;
    let protocol = validate::one_of("protocol", &spec.protocol, &["any", "tcp", "udp"])?;
    let port = spec.port.trim();
    let source = spec.source.trim();
    let port = if port.is_empty() { None } else { Some(validate::port_or_range(port, ':')?) };
    if port.as_deref().is_some_and(|p| p.contains(':')) && protocol == "any" {
        return Err(AppError::invalid("A port range needs a protocol (TCP or UDP)"));
    }
    if port.is_none() && source.is_empty() {
        return Err(AppError::invalid("Enter a port, a source address, or both"));
    }
    let mut cmd = Cmd::new("ufw").arg(action);
    if source.is_empty() {
        // Simple form: `ufw allow 80/tcp`.
        let port = port.unwrap_or_default();
        return Ok(cmd.arg(if protocol == "any" { port } else { format!("{port}/{protocol}") }).build());
    }
    cmd = cmd.lit("from").arg(validate::ip_or_cidr(source)?);
    if let Some(port) = port {
        cmd = cmd.lit("to any port").arg(port);
    }
    if protocol != "any" {
        cmd = cmd.lit("proto").arg(protocol);
    }
    Ok(cmd.build())
}

#[tauri::command]
#[specta::specta]
pub async fn ufw_status(state: State<'_, AppState>) -> AppResult<UfwStatus> {
    let session = state.session()?;
    Ok(parse_ufw_status(&root(&session, "ufw status numbered".into()).await?))
}

/// Enable or disable UFW. `allow_port` adds an allow rule first (keep SSH reachable).
#[tauri::command]
#[specta::specta]
pub async fn ufw_set_enabled(state: State<'_, AppState>, enable: bool, allow_port: Option<u32>) -> AppResult<()> {
    let session = state.session()?;
    let script = if enable {
        match allow_port {
            Some(port) => format!("ufw allow {}/tcp && ufw --force enable", validate::port(port)?),
            None => "ufw --force enable".to_string(),
        }
    } else {
        "ufw disable".to_string()
    };
    root(&session, script).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn ufw_add_rule(state: State<'_, AppState>, spec: UfwRuleSpec) -> AppResult<()> {
    let session = state.session()?;
    root(&session, ufw_rule_command(&spec)?).await?;
    Ok(())
}

/// Delete a rule by its number. Numbers shift after every delete, so the
/// caller re-reads the list before deleting another one.
#[tauri::command]
#[specta::specta]
pub async fn ufw_delete_rule(state: State<'_, AppState>, number: u32) -> AppResult<()> {
    let session = state.session()?;
    if number == 0 {
        return Err(AppError::invalid("Invalid rule number"));
    }
    root(&session, format!("ufw --force delete {number}")).await?;
    Ok(())
}

// ---------------------------------------------------------------- iptables

const TABLES: [&str; 4] = ["filter", "nat", "mangle", "raw"];

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IptRule {
    pub number: u32,
    pub target: String,
    pub protocol: String,
    pub source: String,
    pub destination: String,
    /// Remaining match details (ports, states, comments…).
    pub extra: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IptChain {
    pub name: String,
    /// Built-in chains have a policy; user chains do not.
    pub policy: Option<String>,
    pub rules: Vec<IptRule>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IptTable {
    pub chains: Vec<IptChain>,
    pub raw: String,
}

/// Parse `iptables -t <table> -L -n --line-numbers`.
pub fn parse_iptables(text: &str) -> Vec<IptChain> {
    let mut chains: Vec<IptChain> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Chain ") {
            let name = rest.split_whitespace().next().unwrap_or("").to_string();
            let policy = rest.split_once("(policy ").and_then(|(_, p)| p.split([' ', ')']).next()).map(str::to_string);
            chains.push(IptChain { name, policy, rules: Vec::new() });
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        // num target prot opt source destination [extra…]
        let Some(number) = f.first().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if f.len() < 6 {
            continue;
        }
        if let Some(chain) = chains.last_mut() {
            chain.rules.push(IptRule {
                number,
                target: f[1].to_string(),
                protocol: f[2].to_string(),
                source: f[4].to_string(),
                destination: f[5].to_string(),
                extra: f[6..].join(" "),
            });
        }
    }
    chains
}

fn table(name: &str) -> AppResult<&str> {
    validate::one_of("table", name, &TABLES)
}

fn chain(name: &str) -> AppResult<&str> {
    if !name.is_empty() && name.len() <= 29 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        Ok(name)
    } else {
        Err(AppError::invalid("Invalid chain name"))
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum IptPosition {
    Append,
    /// Insert at the top of the chain.
    Insert,
    /// Insert at a specific rule number.
    Line {
        number: u32,
    },
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IptRuleSpec {
    pub table: String,
    pub chain: String,
    /// `ACCEPT`, `DROP` or `REJECT`.
    pub target: String,
    /// `all`, `tcp`, `udp` or `icmp`.
    pub protocol: String,
    pub source: String,
    pub destination: String,
    /// Destination port or range; only with TCP or UDP.
    pub port: String,
    pub position: IptPosition,
}

pub fn iptables_rule_command(spec: &IptRuleSpec) -> AppResult<String> {
    let mut cmd = Cmd::new("iptables").opt("-t", table(&spec.table)?);
    cmd = match &spec.position {
        IptPosition::Append => cmd.opt("-A", chain(&spec.chain)?),
        IptPosition::Insert => cmd.opt("-I", chain(&spec.chain)?),
        IptPosition::Line { number } => {
            if *number == 0 {
                return Err(AppError::invalid("Rule numbers start at 1"));
            }
            cmd.opt("-I", chain(&spec.chain)?).arg(number.to_string())
        }
    };
    let protocol = validate::one_of("protocol", &spec.protocol, &["all", "tcp", "udp", "icmp"])?;
    if protocol != "all" {
        cmd = cmd.opt("-p", protocol);
    }
    if !spec.source.trim().is_empty() {
        cmd = cmd.opt("-s", validate::ip_or_cidr(spec.source.trim())?);
    }
    if !spec.destination.trim().is_empty() {
        cmd = cmd.opt("-d", validate::ip_or_cidr(spec.destination.trim())?);
    }
    if !spec.port.trim().is_empty() {
        if !matches!(protocol, "tcp" | "udp") {
            return Err(AppError::invalid("A port needs the TCP or UDP protocol"));
        }
        cmd = cmd.opt("--dport", validate::port_or_range(spec.port.trim(), ':')?);
    }
    let target = validate::one_of("target", &spec.target, &["ACCEPT", "DROP", "REJECT"])?;
    Ok(cmd.opt("-j", target).build())
}

#[tauri::command]
#[specta::specta]
pub async fn iptables_list(state: State<'_, AppState>, table_name: String) -> AppResult<IptTable> {
    let session = state.session()?;
    let raw = root(&session, format!("iptables -t {} -L -n --line-numbers", table(&table_name)?)).await?;
    Ok(IptTable { chains: parse_iptables(&raw), raw })
}

#[tauri::command]
#[specta::specta]
pub async fn iptables_add_rule(state: State<'_, AppState>, spec: IptRuleSpec) -> AppResult<()> {
    let session = state.session()?;
    root(&session, iptables_rule_command(&spec)?).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn iptables_delete_rule(state: State<'_, AppState>, table_name: String, chain_name: String, number: u32) -> AppResult<()> {
    let session = state.session()?;
    if number == 0 {
        return Err(AppError::invalid("Rule numbers start at 1"));
    }
    root(&session, format!("iptables -t {} -D {} {number}", table(&table_name)?, q(chain(&chain_name)?))).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn iptables_set_policy(state: State<'_, AppState>, table_name: String, chain_name: String, policy: String) -> AppResult<()> {
    let session = state.session()?;
    let policy = validate::one_of("policy", &policy, &["ACCEPT", "DROP"])?;
    root(&session, format!("iptables -t {} -P {} {policy}", table(&table_name)?, q(chain(&chain_name)?))).await?;
    Ok(())
}

/// Save the current rules so they survive a reboot. Returns where they went.
#[tauri::command]
#[specta::specta]
pub async fn iptables_persist(state: State<'_, AppState>) -> AppResult<String> {
    let session = state.session()?;
    let out = root(
        &session,
        "if command -v netfilter-persistent >/dev/null 2>&1; then netfilter-persistent save >/dev/null 2>&1 && echo 'netfilter-persistent'; \
         elif [ -d /etc/sysconfig ]; then iptables-save > /etc/sysconfig/iptables && echo /etc/sysconfig/iptables; \
         else mkdir -p /etc/iptables && iptables-save > /etc/iptables/rules.v4 && echo /etc/iptables/rules.v4; fi"
            .into(),
    )
    .await?;
    Ok(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ufw_status_numbered() {
        let text = "Status: active\n\n     To                         Action      From\n     --                         ------      ----\n\
[ 1] 22/tcp                     ALLOW IN    Anywhere\n\
[ 2] 80,443/tcp                 ALLOW IN    10.0.0.0/8\n\
[10] 22/tcp (v6)                ALLOW IN    Anywhere (v6)\n\
[11] Anywhere                   DENY IN     203.0.113.7\n";
        let s = parse_ufw_status(text);
        assert!(s.active);
        assert_eq!(s.rules.len(), 4);
        assert_eq!(s.rules[0], UfwRule { number: 1, to: "22/tcp".into(), action: "ALLOW IN".into(), from: "Anywhere".into() });
        assert_eq!(s.rules[1].from, "10.0.0.0/8");
        assert_eq!((s.rules[2].number, s.rules[2].to.as_str(), s.rules[2].from.as_str()), (10, "22/tcp (v6)", "Anywhere (v6)"));
        assert_eq!(s.rules[3].action, "DENY IN");

        let inactive = parse_ufw_status("Status: inactive\n");
        assert!(!inactive.active && inactive.rules.is_empty());
    }

    fn ufw(action: &str, port: &str, protocol: &str, source: &str) -> AppResult<String> {
        ufw_rule_command(&UfwRuleSpec { action: action.into(), port: port.into(), protocol: protocol.into(), source: source.into() })
    }

    #[test]
    fn ufw_rule_commands() {
        assert_eq!(ufw("allow", "80", "tcp", "").unwrap(), "ufw allow 80/tcp");
        assert_eq!(ufw("deny", "53", "any", "").unwrap(), "ufw deny 53");
        assert_eq!(ufw("allow", "8000-8100", "udp", "").unwrap(), "ufw allow 8000:8100/udp");
        assert_eq!(ufw("allow", "22", "tcp", "10.0.0.0/8").unwrap(), "ufw allow from 10.0.0.0/8 to any port 22 proto tcp");
        assert_eq!(ufw("reject", "", "any", "203.0.113.7").unwrap(), "ufw reject from 203.0.113.7");
        assert!(ufw("allow", "8000:8100", "any", "").is_err());
        assert!(ufw("allow", "", "any", "").is_err());
        assert!(ufw("permit", "80", "tcp", "").is_err());
        assert!(ufw("allow", "80; reboot", "tcp", "").is_err());
        assert!(ufw("allow", "80", "tcp", "example.com").is_err());
    }

    #[test]
    fn iptables_listing() {
        let text = "Chain INPUT (policy DROP)\nnum  target     prot opt source               destination\n\
1    ACCEPT     all  --  0.0.0.0/0            0.0.0.0/0            ctstate RELATED,ESTABLISHED\n\
2    ACCEPT     tcp  --  10.0.0.0/8           0.0.0.0/0            tcp dpt:22\n\n\
Chain FORWARD (policy ACCEPT)\nnum  target     prot opt source               destination\n\n\
Chain DOCKER-USER (1 references)\nnum  target     prot opt source               destination\n\
1    RETURN     all  --  0.0.0.0/0            0.0.0.0/0\n";
        let chains = parse_iptables(text);
        assert_eq!(chains.len(), 3);
        assert_eq!(chains[0].policy.as_deref(), Some("DROP"));
        assert_eq!(
            chains[0].rules[1],
            IptRule {
                number: 2,
                target: "ACCEPT".into(),
                protocol: "tcp".into(),
                source: "10.0.0.0/8".into(),
                destination: "0.0.0.0/0".into(),
                extra: "tcp dpt:22".into()
            }
        );
        assert!(chains[1].rules.is_empty());
        assert_eq!(chains[2].policy, None);
        assert_eq!(chains[2].rules[0].target, "RETURN");
    }

    fn ipt(position: IptPosition, protocol: &str, port: &str) -> AppResult<String> {
        iptables_rule_command(&IptRuleSpec {
            table: "filter".into(),
            chain: "INPUT".into(),
            target: "ACCEPT".into(),
            protocol: protocol.into(),
            source: "10.0.0.0/8".into(),
            destination: String::new(),
            port: port.into(),
            position,
        })
    }

    #[test]
    fn iptables_rule_commands() {
        assert_eq!(
            ipt(IptPosition::Append, "tcp", "443").unwrap(),
            "iptables -t filter -A INPUT -p tcp -s 10.0.0.0/8 --dport 443 -j ACCEPT"
        );
        assert_eq!(ipt(IptPosition::Insert, "all", "").unwrap(), "iptables -t filter -I INPUT -s 10.0.0.0/8 -j ACCEPT");
        assert_eq!(
            ipt(IptPosition::Line { number: 3 }, "udp", "5000-5100").unwrap(),
            "iptables -t filter -I INPUT 3 -p udp -s 10.0.0.0/8 --dport 5000:5100 -j ACCEPT"
        );
        assert!(ipt(IptPosition::Append, "all", "80").is_err());
        assert!(ipt(IptPosition::Line { number: 0 }, "tcp", "80").is_err());
        assert!(chain("INPUT; reboot").is_err());
        assert!(table("security").is_err());
    }
}
