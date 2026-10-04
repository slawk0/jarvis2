//! CrowdSec management through `cscli … -o json`.
//!
//! `cscli` can run natively (as root), inside a Docker container, or behind a
//! custom command prefix (e.g. podman). The per-profile connection config
//! selects which; "auto" detects native first, then a container.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use tauri::State;

use crate::docker::spec::split_words;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::shell::{q, validate, Cmd};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;
use crate::store::profile_data::DataKey;

const WHITELIST_DIR: &str = "/etc/crowdsec/parsers/s02-enrich";
pub const MANAGED_WHITELIST: &str = "jarvis-whitelist.yaml";

/// Stored per profile (written by the UI).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct StoredConfig {
    mode: String,
    container: String,
    prefix: String,
}

/// How `cscli` is reached on this server.
#[derive(Debug, Clone, PartialEq)]
pub enum Access {
    Native,
    Docker { container: String },
    Custom { words: Vec<String> },
}

impl Access {
    /// The command prefix as shell words (without sudo).
    fn words(&self) -> Vec<String> {
        match self {
            Access::Native => vec!["cscli".into()],
            Access::Docker { container } => {
                vec!["docker".into(), "exec".into(), container.clone(), "cscli".into()]
            }
            Access::Custom { words } => words.clone(),
        }
    }

    /// Prefix for running another program next to cscli (cat, tail) in the same place.
    fn shell_words(&self, program: &str) -> Vec<String> {
        match self {
            Access::Native => vec![program.to_string()],
            Access::Docker { container } => {
                vec!["docker".into(), "exec".into(), container.clone(), program.to_string()]
            }
            // "podman exec name cscli" → "podman exec name <program>"
            Access::Custom { words } => {
                let mut w = words.clone();
                w.pop();
                w.push(program.to_string());
                w
            }
        }
    }
}

struct Cscli<'a> {
    session: &'a Session,
    access: Access,
    sudo: bool,
}

impl Cscli<'_> {
    async fn exec(&self, args: Cmd) -> AppResult<crate::ssh::session::Output> {
        let line = format!("{} {}", Cmd::from_words(self.access.words()).build(), args.build());
        self.session.exec(Exec::new(line).sudo_if(self.sudo).secs(90)).await
    }

    async fn run(&self, args: Cmd) -> AppResult<String> {
        self.exec(args).await?.into_stdout()
    }

    async fn json(&self, args: Cmd) -> AppResult<Value> {
        let out = self.run(args.lit("-o json")).await?;
        let text = out.trim();
        if text.is_empty() || text == "null" {
            return Ok(Value::Null);
        }
        serde_json::from_str(text).map_err(|e| AppError::parse(format!("cscli output: {e}")))
    }

    /// Run a non-cscli program where CrowdSec's files live.
    async fn tool(&self, program: &'static str, args: Cmd, stdin: Option<String>) -> AppResult<String> {
        let line = format!("{} {}", Cmd::from_words(self.access.shell_words(program)).build(), args.build());
        let mut exec = Exec::new(line).sudo_if(self.sudo).secs(60);
        if let Some(data) = stdin {
            exec = exec.stdin(data);
        }
        self.session.exec(exec).await?.into_stdout()
    }
}

async fn resolve(state: &AppState, session: &Session) -> AppResult<Access> {
    let config: StoredConfig = state.data.get_as(&session.profile.id, DataKey::CrowdsecConfig)?;
    match config.mode.as_str() {
        "native" => Ok(Access::Native),
        "docker" => Ok(Access::Docker { container: validate::name("container", config.container.trim())?.to_string() }),
        "custom" => {
            let words = split_words(&config.prefix)?;
            if words.is_empty() {
                return Err(AppError::invalid("The custom cscli command is empty"));
            }
            for word in &words {
                validate::single_line("command", word)?;
            }
            Ok(Access::Custom { words })
        }
        _ => detect(session).await?.ok_or_else(|| AppError::new(ErrorCode::DependencyMissing, "CrowdSec")),
    }
}

/// Find CrowdSec: native `cscli` first, then a running container with a crowdsec image.
async fn detect(session: &Session) -> AppResult<Option<Access>> {
    if session.exec(Exec::new("command -v cscli >/dev/null 2>&1")).await?.success() {
        return Ok(Some(Access::Native));
    }
    if !session.exec(Exec::new("command -v docker >/dev/null 2>&1")).await?.success() {
        return Ok(None);
    }
    let out = crate::docker::run(session, "docker ps --format '{{.Names}} {{.Image}}'").await.unwrap_or_default();
    Ok(out.lines().find_map(|line| {
        let (name, image) = line.split_once(' ')?;
        image.contains("crowdsec").then(|| Access::Docker { container: name.to_string() })
    }))
}

async fn cscli<'a>(state: &AppState, session: &'a Session) -> AppResult<Cscli<'a>> {
    let access = resolve(state, session).await?;
    let sudo = match &access {
        Access::Native => true,
        Access::Docker { .. } => crate::docker::needs_sudo(session).await?,
        Access::Custom { .. } => false,
    };
    Ok(Cscli { session, access, sudo })
}

fn text(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn array(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

// ---------------------------------------------------------------- status

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CrowdsecStatus {
    pub installed: bool,
    /// `native`, `docker` or `custom`.
    pub mode: String,
    pub container: String,
    pub version: String,
    /// `None` when not a systemd-managed native install.
    pub service_active: Option<bool>,
}

/// Detect CrowdSec and test the connection (`cscli version`).
#[tauri::command]
#[specta::specta]
pub async fn crowdsec_status(state: State<'_, AppState>) -> AppResult<CrowdsecStatus> {
    let session = state.session()?;
    let not_installed =
        CrowdsecStatus { installed: false, mode: String::new(), container: String::new(), version: String::new(), service_active: None };
    let cli = match cscli(&state, &session).await {
        Ok(cli) => cli,
        Err(e) if e.is(ErrorCode::DependencyMissing) => return Ok(not_installed),
        Err(e) => return Err(e),
    };
    let out = cli.exec(Cmd::new("version").lit("2>&1")).await?;
    if out.code == 127 {
        return Ok(not_installed);
    }
    let combined = out.combined();
    if !out.success() {
        return Err(AppError::new(ErrorCode::CommandFailed, combined));
    }
    let version = combined.lines().find_map(|l| l.split_once("version:").map(|(_, v)| v.trim().to_string())).unwrap_or_default();
    let (mode, container) = match &cli.access {
        Access::Native => ("native", String::new()),
        Access::Docker { container } => ("docker", container.clone()),
        Access::Custom { .. } => ("custom", String::new()),
    };
    let service_active = if cli.access == Access::Native {
        Some(session.exec(Exec::new("systemctl is-active crowdsec >/dev/null 2>&1")).await?.success())
    } else {
        None
    };
    Ok(CrowdsecStatus { installed: true, mode: mode.to_string(), container, version, service_active })
}

// ---------------------------------------------------------------- decisions

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub id: String,
    pub value: String,
    pub scope: String,
    /// `ban`, `captcha`, …
    pub kind: String,
    pub origin: String,
    pub reason: String,
    pub country: String,
    pub as_name: String,
    pub duration: String,
    pub until: String,
}

/// `cscli decisions list -o json`: alerts, each with its decisions and source.
pub fn parse_decisions(value: &Value) -> Vec<Decision> {
    array(value)
        .iter()
        .flat_map(|alert| {
            let source = alert.get("source").cloned().unwrap_or(Value::Null);
            array(alert.get("decisions").unwrap_or(&Value::Null)).into_iter().map(move |d| Decision {
                id: text(&d, "id"),
                value: text(&d, "value"),
                scope: text(&d, "scope"),
                kind: text(&d, "type"),
                origin: text(&d, "origin"),
                reason: text(&d, "scenario"),
                country: text(&source, "cn"),
                as_name: text(&source, "as_name"),
                duration: text(&d, "duration"),
                until: text(&d, "until"),
            })
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_decisions(state: State<'_, AppState>) -> AppResult<Vec<Decision>> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    Ok(parse_decisions(&cli.json(Cmd::new("decisions").lit("list")).await?))
}

fn duration(value: &str) -> AppResult<&str> {
    let value = value.trim();
    let ok = !value.is_empty()
        && value.len() <= 16
        && value.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'h' | b'm' | b's' | b'd'))
        && value.bytes().next().is_some_and(|b| b.is_ascii_digit())
        && !value.bytes().last().is_some_and(|b| b.is_ascii_digit());
    if ok {
        Ok(value)
    } else {
        Err(AppError::invalid("Invalid duration (examples: 4h, 30m, 7d)"))
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BanRequest {
    /// `ip` or `range`.
    pub scope: String,
    pub value: String,
    pub duration: String,
    pub reason: String,
}

/// CrowdSec durations have no day unit: `7d` becomes `168h`.
pub fn normalize_duration(value: &str) -> AppResult<String> {
    let value = duration(value)?;
    match value.strip_suffix('d') {
        Some(days) if days.bytes().all(|b| b.is_ascii_digit()) => Ok(format!("{}h", days.parse::<u64>().unwrap_or(1) * 24)),
        _ if value.contains('d') => Err(AppError::invalid("Use days on their own, e.g. 7d")),
        _ => Ok(value.to_string()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_ban(state: State<'_, AppState>, request: BanRequest) -> AppResult<()> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    let mut cmd = Cmd::new("decisions").lit("add --type ban");
    cmd = match request.scope.as_str() {
        "range" => {
            let value = validate::ip_or_cidr(request.value.trim())?;
            if !value.contains('/') {
                return Err(AppError::invalid("A range needs a prefix, e.g. 203.0.113.0/24"));
            }
            cmd.opt("--range", value)
        }
        _ => cmd.opt("--ip", validate::ip(request.value.trim())?),
    };
    cmd = cmd.opt("--duration", normalize_duration(&request.duration)?);
    let reason = validate::single_line("reason", request.reason.trim())?;
    cmd = cmd.opt("--reason", if reason.is_empty() { "manual ban from Jarvis" } else { reason });
    cli.run(cmd).await?;
    Ok(())
}

/// Remove one decision by id, or all of them when `id` is `None`.
#[tauri::command]
#[specta::specta]
pub async fn crowdsec_unban(state: State<'_, AppState>, id: Option<String>) -> AppResult<()> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    let cmd = Cmd::new("decisions").lit("delete");
    let cmd = match id {
        Some(id) if id.bytes().all(|b| b.is_ascii_digit()) && !id.is_empty() => cmd.opt("--id", id),
        Some(_) => return Err(AppError::invalid("Invalid decision id")),
        None => cmd.lit("--all"),
    };
    cli.run(cmd).await?;
    Ok(())
}

// ---------------------------------------------------------------- alerts

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: String,
    pub created_at: String,
    pub scenario: String,
    pub source: String,
    pub country: String,
    pub as_name: String,
    pub events: u32,
    pub message: String,
}

pub fn parse_alerts(value: &Value) -> Vec<Alert> {
    array(value)
        .iter()
        .map(|a| {
            let source = a.get("source").cloned().unwrap_or(Value::Null);
            Alert {
                id: text(a, "id"),
                created_at: text(a, "created_at"),
                scenario: text(a, "scenario"),
                source: {
                    let value = text(&source, "value");
                    if value.is_empty() {
                        text(&source, "ip")
                    } else {
                        value
                    }
                },
                country: text(&source, "cn"),
                as_name: text(&source, "as_name"),
                events: a.get("events_count").and_then(Value::as_u64).unwrap_or(0) as u32,
                message: text(a, "message"),
            }
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_alerts(state: State<'_, AppState>) -> AppResult<Vec<Alert>> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    Ok(parse_alerts(&cli.json(Cmd::new("alerts").lit("list -l 300")).await?))
}

/// Full alert as pretty-printed JSON (events, decisions, metadata).
#[tauri::command]
#[specta::specta]
pub async fn crowdsec_alert_detail(state: State<'_, AppState>, id: String) -> AppResult<String> {
    let session = state.session()?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppError::invalid("Invalid alert id"));
    }
    let cli = cscli(&state, &session).await?;
    let value = cli.json(Cmd::new("alerts").lit("inspect").arg(&id).lit("-d")).await?;
    Ok(serde_json::to_string_pretty(&value)?)
}

// ---------------------------------------------------------------- bouncers

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Bouncer {
    pub name: String,
    pub ip: String,
    pub kind: String,
    pub version: String,
    pub last_pull: String,
    pub revoked: bool,
}

pub fn parse_bouncers(value: &Value) -> Vec<Bouncer> {
    array(value)
        .iter()
        .map(|b| Bouncer {
            name: text(b, "name"),
            ip: text(b, "ip_address"),
            kind: text(b, "type"),
            version: text(b, "version"),
            last_pull: text(b, "last_pull"),
            revoked: b.get("revoked").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_bouncers(state: State<'_, AppState>) -> AppResult<Vec<Bouncer>> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    Ok(parse_bouncers(&cli.json(Cmd::new("bouncers").lit("list")).await?))
}

/// Register a bouncer; returns its API key (shown once).
#[tauri::command]
#[specta::specta]
pub async fn crowdsec_bouncer_add(state: State<'_, AppState>, name: String) -> AppResult<String> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    let out = cli.run(Cmd::new("bouncers").lit("add").arg(validate::slug("bouncer name", &name)?).lit("-o raw")).await?;
    Ok(out.trim().to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_bouncer_delete(state: State<'_, AppState>, name: String) -> AppResult<()> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    cli.run(Cmd::new("bouncers").lit("delete").arg(validate::name("bouncer name", &name)?)).await?;
    Ok(())
}

/// Remove bouncers that have not pulled for a long time.
#[tauri::command]
#[specta::specta]
pub async fn crowdsec_bouncers_prune(state: State<'_, AppState>) -> AppResult<String> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    Ok(cli.exec(Cmd::new("bouncers").lit("prune --force 2>&1")).await?.combined())
}

// ---------------------------------------------------------------- metrics

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Acquisition {
    /// e.g. `file:/var/log/nginx/access.log`.
    pub source: String,
    pub read: u64,
    pub parsed: u64,
    pub unparsed: u64,
}

/// The acquisition section of `cscli metrics -o json`.
pub fn parse_acquisition(value: &Value) -> Vec<Acquisition> {
    let Some(map) = value.get("acquisition").and_then(Value::as_object) else {
        return Vec::new();
    };
    let count = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0);
    let mut list: Vec<Acquisition> = map
        .iter()
        .map(|(source, stats)| Acquisition {
            source: source.clone(),
            read: count(stats, "reads"),
            parsed: count(stats, "parsed"),
            unparsed: count(stats, "unparsed"),
        })
        .collect();
    list.sort_by_key(|a| std::cmp::Reverse(a.read));
    list
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_metrics(state: State<'_, AppState>) -> AppResult<Vec<Acquisition>> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    Ok(parse_acquisition(&cli.json(Cmd::new("metrics")).await?))
}

/// Last lines of an acquisition source that is a file.
#[tauri::command]
#[specta::specta]
pub async fn crowdsec_source_tail(state: State<'_, AppState>, source: String, lines: u32) -> AppResult<String> {
    let session = state.session()?;
    let path = source.strip_prefix("file:").unwrap_or(&source);
    validate::abs_path(path)?;
    let cli = cscli(&state, &session).await?;
    cli.tool("tail", Cmd::new("-n").arg(lines.clamp(10, 2000).to_string()).arg(path), None).await
}

// ---------------------------------------------------------------- hub

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HubItem {
    /// `collections`, `parsers`, `scenarios` or `postoverflows`.
    pub kind: String,
    pub name: String,
    pub status: String,
    pub local_version: String,
    pub description: String,
    pub installed: bool,
}

const HUB_KINDS: [&str; 4] = ["collections", "parsers", "scenarios", "postoverflows"];

pub fn parse_hub(value: &Value) -> Vec<HubItem> {
    HUB_KINDS
        .iter()
        .flat_map(|kind| {
            array(value.get(*kind).unwrap_or(&Value::Null)).into_iter().map(move |item| {
                let status = text(&item, "status");
                HubItem {
                    kind: kind.to_string(),
                    name: text(&item, "name"),
                    local_version: text(&item, "local_version"),
                    description: text(&item, "description"),
                    installed: status.contains("enabled") || status.contains("installed") && !status.contains("disabled"),
                    status,
                }
            })
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_hub(state: State<'_, AppState>) -> AppResult<Vec<HubItem>> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    Ok(parse_hub(&cli.json(Cmd::new("hub").lit("list -a")).await?))
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum HubAction {
    Install,
    Upgrade,
    Remove,
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_hub_action(state: State<'_, AppState>, kind: String, name: String, action: HubAction) -> AppResult<String> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    let kind = validate::one_of("hub item type", &kind, &HUB_KINDS)?;
    let verb = match action {
        HubAction::Install => "install",
        HubAction::Upgrade => "upgrade",
        HubAction::Remove => "remove",
    };
    let out = cli.exec(Cmd::from_words([kind, verb]).arg(validate::name("hub item", &name)?).lit("2>&1")).await?;
    if !out.success() {
        return Err(AppError::new(ErrorCode::CommandFailed, out.combined()));
    }
    reload(&cli).await;
    Ok(out.combined())
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_hub_update(state: State<'_, AppState>) -> AppResult<String> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    let out = cli.exec(Cmd::new("hub").lit("update 2>&1")).await?;
    if !out.success() {
        return Err(AppError::new(ErrorCode::CommandFailed, out.combined()));
    }
    Ok(out.combined())
}

// ---------------------------------------------------------------- whitelists

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WhitelistFile {
    pub path: String,
    pub content: String,
    /// The file Jarvis writes; the others are read-only here.
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Allowlist {
    pub name: String,
    pub description: String,
    pub items: Vec<String>,
}

pub fn parse_allowlists(value: &Value) -> Vec<Allowlist> {
    array(value)
        .iter()
        .map(|list| Allowlist {
            name: text(list, "name"),
            description: text(list, "description"),
            items: array(list.get("items").unwrap_or(&Value::Null))
                .iter()
                .map(|item| text(item, "value"))
                .filter(|v| !v.is_empty())
                .collect(),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Whitelists {
    /// Parser whitelist YAML files (parsed by the UI).
    pub files: Vec<WhitelistFile>,
    /// LAPI allowlists (CrowdSec 1.6.8+; empty on older versions).
    pub allowlists: Vec<Allowlist>,
}

#[tauri::command]
#[specta::specta]
pub async fn crowdsec_whitelists(state: State<'_, AppState>) -> AppResult<Whitelists> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    const MARK: &str = "#jarvis-file:";
    let script = format!(
        "for f in {WHITELIST_DIR}/*.yaml {WHITELIST_DIR}/*.yml; do [ -f \"$f\" ] || continue; echo \"{MARK}$f\"; cat \"$f\"; echo; done; true"
    );
    let out = cli.tool("sh", Cmd::new("-c").arg(&script), None).await?;
    let mut files: Vec<WhitelistFile> = Vec::new();
    for line in out.lines() {
        if let Some(path) = line.strip_prefix(MARK) {
            files.push(WhitelistFile { managed: path.ends_with(MANAGED_WHITELIST), path: path.to_string(), content: String::new() });
        } else if let Some(file) = files.last_mut() {
            file.content.push_str(line);
            file.content.push('\n');
        }
    }
    // Only whitelist parsers are relevant here.
    files.retain(|f| f.content.contains("whitelist"));
    let allowlists = match cli.json(Cmd::new("allowlists").lit("list")).await {
        Ok(value) => {
            let mut lists = parse_allowlists(&value);
            for list in &mut lists {
                if let Ok(detail) = cli.json(Cmd::new("allowlists").lit("inspect").arg(&list.name)).await {
                    if let Some(found) = parse_allowlists(&Value::Array(vec![detail])).pop() {
                        list.items = found.items;
                    }
                }
            }
            lists
        }
        Err(_) => Vec::new(),
    };
    Ok(Whitelists { files, allowlists })
}

/// YAML for the Jarvis-managed parser whitelist.
pub fn render_whitelist(ips: &[String], cidrs: &[String]) -> AppResult<String> {
    let mut yaml = String::from(
        "# Managed by Jarvis Server Manager\nname: jarvis/whitelist\ndescription: \"Whitelist managed by Jarvis\"\nwhitelist:\n  reason: \"Managed by Jarvis\"\n",
    );
    if !ips.is_empty() {
        yaml.push_str("  ip:\n");
        for ip in ips {
            yaml.push_str(&format!("    - \"{}\"\n", validate::ip(ip.trim())?));
        }
    }
    if !cidrs.is_empty() {
        yaml.push_str("  cidr:\n");
        for cidr in cidrs {
            let cidr = validate::ip_or_cidr(cidr.trim())?;
            if !cidr.contains('/') {
                return Err(AppError::invalid(format!("{cidr} is not a CIDR range")));
            }
            yaml.push_str(&format!("    - \"{cidr}\"\n"));
        }
    }
    Ok(yaml)
}

async fn reload(cli: &Cscli<'_>) {
    let script = match &cli.access {
        Access::Native => "systemctl reload crowdsec 2>/dev/null || systemctl restart crowdsec".to_string(),
        Access::Docker { container } => format!("docker kill --signal=HUP {}", q(container)),
        Access::Custom { .. } => return,
    };
    let _ = cli.session.exec(Exec::new(script).sudo_if(cli.sudo).secs(60)).await;
}

/// Replace the Jarvis-managed whitelist with these entries and reload CrowdSec.
#[tauri::command]
#[specta::specta]
pub async fn crowdsec_whitelist_save(state: State<'_, AppState>, ips: Vec<String>, cidrs: Vec<String>) -> AppResult<()> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    let file = format!("{WHITELIST_DIR}/{MANAGED_WHITELIST}");
    if ips.is_empty() && cidrs.is_empty() {
        cli.tool("rm", Cmd::new("-f").arg(&file), None).await?;
    } else {
        let yaml = render_whitelist(&ips, &cidrs)?;
        let script = format!("cat > {}", q(&file));
        // `docker exec` needs -i to pass stdin through.
        match &cli.access {
            Access::Docker { container } => {
                let line = Cmd::new("docker").lit("exec -i").arg(container).lit("sh -c").arg(&script).build();
                session.exec(Exec::new(line).sudo_if(cli.sudo).stdin(yaml)).await?.into_stdout()?;
            }
            _ => {
                cli.tool("sh", Cmd::new("-c").arg(&script), Some(yaml)).await?;
            }
        }
    }
    reload(&cli).await;
    Ok(())
}

/// Add or remove a value in a LAPI allowlist.
#[tauri::command]
#[specta::specta]
pub async fn crowdsec_allowlist_edit(state: State<'_, AppState>, name: String, value: String, add: bool) -> AppResult<()> {
    let session = state.session()?;
    let cli = cscli(&state, &session).await?;
    let cmd = Cmd::new("allowlists")
        .lit(if add { "add" } else { "remove" })
        .arg(validate::slug("allowlist name", &name)?)
        .arg(validate::ip_or_cidr(value.trim())?);
    cli.run(cmd).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn access_prefixes() {
        assert_eq!(Access::Native.words(), vec!["cscli"]);
        let docker = Access::Docker { container: "crowdsec".into() };
        assert_eq!(docker.words(), vec!["docker", "exec", "crowdsec", "cscli"]);
        assert_eq!(docker.shell_words("tail"), vec!["docker", "exec", "crowdsec", "tail"]);
        let custom = Access::Custom { words: vec!["podman".into(), "exec".into(), "cs".into(), "cscli".into()] };
        assert_eq!(custom.shell_words("cat"), vec!["podman", "exec", "cs", "cat"]);
    }

    #[test]
    fn decisions_are_flattened_from_alerts() {
        let value = json!([
            {"id": 12, "source": {"cn": "FR", "as_name": "OVH SAS", "ip": "203.0.113.9"},
             "decisions": [
                {"id": 501, "value": "203.0.113.9", "scope": "Ip", "type": "ban", "origin": "crowdsec", "scenario": "crowdsecurity/ssh-bf", "duration": "3h58m", "until": "2026-10-04T02:00:00Z"},
                {"id": 502, "value": "203.0.113.0/24", "scope": "Range", "type": "ban", "origin": "cscli", "scenario": "manual", "duration": "24h"}
             ]},
            {"id": 13, "decisions": null}
        ]);
        let d = parse_decisions(&value);
        assert_eq!(d.len(), 2);
        assert_eq!(
            (d[0].id.as_str(), d[0].value.as_str(), d[0].country.as_str(), d[0].as_name.as_str()),
            ("501", "203.0.113.9", "FR", "OVH SAS")
        );
        assert_eq!((d[1].scope.as_str(), d[1].origin.as_str(), d[1].until.as_str()), ("Range", "cscli", ""));
        assert!(parse_decisions(&Value::Null).is_empty());
    }

    #[test]
    fn alerts_bouncers_metrics_hub() {
        let alerts = parse_alerts(
            &json!([{"id": 7, "created_at": "2026-10-03T10:00:00Z", "scenario": "crowdsecurity/http-probing", "events_count": 14, "message": "Ip 198.51.100.4 performed probing", "source": {"value": "198.51.100.4", "cn": "US"}}]),
        );
        assert_eq!((alerts[0].id.as_str(), alerts[0].source.as_str(), alerts[0].events), ("7", "198.51.100.4", 14));

        let bouncers = parse_bouncers(
            &json!([{"name": "fw", "ip_address": "127.0.0.1", "type": "crowdsec-firewall-bouncer", "version": "v0.0.31", "last_pull": "2026-10-03T10:00:00Z", "revoked": false}]),
        );
        assert_eq!((bouncers[0].name.as_str(), bouncers[0].kind.as_str(), bouncers[0].revoked), ("fw", "crowdsec-firewall-bouncer", false));

        let acq = parse_acquisition(
            &json!({"acquisition": {"file:/var/log/auth.log": {"reads": 100, "parsed": 20, "unparsed": 80}, "file:/var/log/nginx/access.log": {"reads": 5000, "parsed": 4900, "unparsed": 100, "pour": 4900}}}),
        );
        assert_eq!(acq[0].source, "file:/var/log/nginx/access.log");
        assert_eq!((acq[1].read, acq[1].parsed, acq[1].unparsed), (100, 20, 80));
        assert!(parse_acquisition(&json!({})).is_empty());

        let hub = parse_hub(
            &json!({"collections": [{"name": "crowdsecurity/nginx", "status": "enabled", "local_version": "0.2", "description": "nginx support"}], "parsers": [{"name": "crowdsecurity/whitelists", "status": "disabled"}], "scenarios": null}),
        );
        assert_eq!(hub.len(), 2);
        assert!(hub[0].installed);
        assert!(!hub[1].installed);
        assert_eq!(hub[1].kind, "parsers");
    }

    #[test]
    fn durations() {
        assert_eq!(normalize_duration("4h").unwrap(), "4h");
        assert_eq!(normalize_duration("7d").unwrap(), "168h");
        assert_eq!(normalize_duration("1h30m").unwrap(), "1h30m");
        assert!(normalize_duration("").is_err());
        assert!(normalize_duration("4").is_err());
        assert!(normalize_duration("h4").is_err());
        assert!(normalize_duration("4h; reboot").is_err());
        assert!(normalize_duration("1d12h").is_err());
    }

    #[test]
    fn whitelist_yaml() {
        let yaml = render_whitelist(&["10.0.0.5".into()], &["192.168.0.0/16".into()]).unwrap();
        assert!(yaml.starts_with("# Managed by Jarvis"));
        assert!(yaml.contains("  ip:\n    - \"10.0.0.5\"\n"));
        assert!(yaml.contains("  cidr:\n    - \"192.168.0.0/16\"\n"));
        assert!(render_whitelist(&["not-an-ip".into()], &[]).is_err());
        assert!(render_whitelist(&[], &["10.0.0.1".into()]).is_err());
    }

    #[test]
    fn allowlists() {
        let lists = parse_allowlists(
            &json!([{"name": "office", "description": "Office IPs", "items": [{"value": "203.0.113.1"}, {"value": "203.0.113.0/28"}]}]),
        );
        assert_eq!(lists[0].items, vec!["203.0.113.1", "203.0.113.0/28"]);
    }
}
