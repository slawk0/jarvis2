//! systemd services and timers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::JobMeta;
use crate::shell::{q, validate};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

const UNIT_DIR: &str = "/etc/systemd/system";
/// First line of every unit file Jarvis writes; only such units may be deleted.
pub const MANAGED_MARK: &str = "# Managed by Jarvis Server Manager";

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServiceUnit {
    pub name: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    /// `enabled`, `disabled`, `static`, `masked`, … or empty when unknown.
    pub enabled: String,
    pub description: String,
}

/// Merge `list-units` (runtime state) with `list-unit-files` (enablement).
pub fn parse_services(units: &str, unit_files: &str) -> Vec<ServiceUnit> {
    let mut map: BTreeMap<String, ServiceUnit> = BTreeMap::new();
    for line in units.lines() {
        // Failed units are prefixed with a bullet in some systemd versions.
        let line = line.trim_start_matches(['●', '*', ' ']);
        let mut f = line.split_whitespace();
        let (Some(name), Some(load), Some(active), Some(sub)) = (f.next(), f.next(), f.next(), f.next()) else {
            continue;
        };
        if !name.ends_with(".service") {
            continue;
        }
        map.insert(
            name.to_string(),
            ServiceUnit {
                name: name.to_string(),
                load: load.to_string(),
                active: active.to_string(),
                sub: sub.to_string(),
                enabled: String::new(),
                description: f.collect::<Vec<_>>().join(" "),
            },
        );
    }
    for line in unit_files.lines() {
        let mut f = line.split_whitespace();
        let (Some(name), Some(state)) = (f.next(), f.next()) else {
            continue;
        };
        // Templates (`foo@.service`) are not runnable units themselves.
        if !name.ends_with(".service") || name.contains("@.") {
            continue;
        }
        map.entry(name.to_string()).and_modify(|u| u.enabled = state.to_string()).or_insert_with(|| ServiceUnit {
            name: name.to_string(),
            load: "loaded".into(),
            active: "inactive".into(),
            sub: "dead".into(),
            enabled: state.to_string(),
            description: String::new(),
        });
    }
    map.into_values().collect()
}

pub async fn list_services(session: &Session) -> AppResult<Vec<ServiceUnit>> {
    let out = session
        .run(
            "systemctl list-units --type=service --all --no-legend --no-pager --plain; echo '#files'; \
             systemctl list-unit-files --type=service --no-legend --no-pager --plain",
        )
        .await?;
    let (units, files) = out.split_once("#files\n").unwrap_or((&out, ""));
    Ok(parse_services(units, files))
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UnitAction {
    Start,
    Stop,
    Restart,
    Reload,
    Enable,
    Disable,
}

impl UnitAction {
    fn verb(self) -> &'static str {
        match self {
            UnitAction::Start => "start",
            UnitAction::Stop => "stop",
            UnitAction::Restart => "restart",
            UnitAction::Reload => "reload",
            UnitAction::Enable => "enable",
            UnitAction::Disable => "disable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RestartPolicy {
    Always,
    OnFailure,
    No,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSpec {
    /// Unit name without the `.service` suffix.
    pub name: String,
    pub description: String,
    pub exec_start: String,
    pub user: String,
    pub restart: RestartPolicy,
    pub working_dir: String,
    /// `KEY=value` pairs.
    pub environment: Vec<(String, String)>,
    pub enable: bool,
    pub start: bool,
}

fn unit_value<'a>(what: &str, value: &'a str) -> AppResult<&'a str> {
    validate::single_line(what, value.trim())
}

pub fn render_service(spec: &ServiceSpec) -> AppResult<String> {
    validate::slug("service name", &spec.name)?;
    let exec = unit_value("command", &spec.exec_start)?;
    if exec.is_empty() {
        return Err(AppError::invalid("ExecStart is required"));
    }
    let mut unit = format!("{MANAGED_MARK}\n[Unit]\n");
    let description = unit_value("description", &spec.description)?;
    unit.push_str(&format!(
        "Description={}\nAfter=network.target\n\n[Service]\nType=simple\n",
        if description.is_empty() { &spec.name } else { description }
    ));
    unit.push_str(&format!("ExecStart={exec}\n"));
    let user = spec.user.trim();
    if !user.is_empty() {
        unit.push_str(&format!("User={}\n", validate::username(user)?));
    }
    let dir = spec.working_dir.trim();
    if !dir.is_empty() {
        unit.push_str(&format!("WorkingDirectory={}\n", validate::abs_path(dir)?));
    }
    for (key, value) in &spec.environment {
        let value = unit_value("environment value", value)?.replace('\\', "\\\\").replace('"', "\\\"");
        unit.push_str(&format!("Environment=\"{}={value}\"\n", validate::env_key(key.trim())?));
    }
    unit.push_str(match spec.restart {
        RestartPolicy::Always => "Restart=always\nRestartSec=5\n",
        RestartPolicy::OnFailure => "Restart=on-failure\nRestartSec=5\n",
        RestartPolicy::No => "Restart=no\n",
    });
    unit.push_str("\n[Install]\nWantedBy=multi-user.target\n");
    Ok(unit)
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TimerSpec {
    /// Base name; creates `<name>.timer` and `<name>.service`.
    pub name: String,
    pub description: String,
    /// systemd calendar expression, e.g. `daily` or `*-*-* 03:00:00`.
    pub on_calendar: String,
    pub command: String,
    pub user: String,
    /// Run a missed job after boot.
    pub persistent: bool,
}

pub fn render_timer(spec: &TimerSpec) -> AppResult<(String, String)> {
    validate::slug("timer name", &spec.name)?;
    let calendar = unit_value("schedule", &spec.on_calendar)?;
    let command = unit_value("command", &spec.command)?;
    if calendar.is_empty() || command.is_empty() {
        return Err(AppError::invalid("Schedule and command are required"));
    }
    let description = unit_value("description", &spec.description)?;
    let description = if description.is_empty() { &spec.name } else { description };
    let mut service = format!("{MANAGED_MARK}\n[Unit]\nDescription={description}\n\n[Service]\nType=oneshot\nExecStart={command}\n");
    let user = spec.user.trim();
    if !user.is_empty() {
        service.push_str(&format!("User={}\n", validate::username(user)?));
    }
    let timer = format!(
        "{MANAGED_MARK}\n[Unit]\nDescription={description}\n\n[Timer]\nOnCalendar={calendar}\nPersistent={}\n\n[Install]\nWantedBy=timers.target\n",
        if spec.persistent { "true" } else { "false" }
    );
    Ok((service, timer))
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TimerUnit {
    pub unit: String,
    pub description: String,
    /// The service this timer starts.
    pub activates: String,
    /// Unix seconds; `None` when not scheduled / never ran.
    pub next: Option<i64>,
    pub last: Option<i64>,
    pub enabled: String,
    pub active: String,
}

#[derive(Deserialize)]
struct TimerTimes {
    unit: String,
    /// Microseconds since the epoch; `null` or 0 when not scheduled / never ran.
    #[serde(default)]
    next: Option<u64>,
    #[serde(default)]
    last: Option<u64>,
}

/// Combine `systemctl show '*.timer'` (blank-line separated property blocks)
/// with `systemctl list-timers --output=json`, which is the only place that
/// reports next/last runs as plain microsecond timestamps for both calendar
/// and monotonic timers. Without JSON support (systemd < 246) times are unknown.
pub fn parse_timers(show: &str, times_json: &str) -> Vec<TimerUnit> {
    let times: Vec<TimerTimes> = serde_json::from_str(times_json.trim()).unwrap_or_default();
    let stamp = |micros: Option<u64>| micros.filter(|m| *m > 0).map(|m| (m / 1_000_000) as i64);
    show.split(
        "

",
    )
    .filter_map(|block| {
        let get = |key: &str| block.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('='))).unwrap_or("").to_string();
        let unit = get("Id");
        if !unit.ends_with(".timer") {
            return None;
        }
        let time = times.iter().find(|t| t.unit == unit);
        Some(TimerUnit {
            description: get("Description"),
            activates: get("Unit"),
            next: time.and_then(|t| stamp(t.next)),
            last: time.and_then(|t| stamp(t.last)),
            enabled: get("UnitFileState"),
            active: get("ActiveState"),
            unit,
        })
    })
    .collect()
}

pub async fn list_timers(session: &Session) -> AppResult<Vec<TimerUnit>> {
    let out = session
        .run(
            "systemctl show --no-pager -p Id,Description,Unit,UnitFileState,ActiveState '*.timer'; echo '#times';              systemctl list-timers --all --no-pager --output=json 2>/dev/null; true",
        )
        .await?;
    let (show, times) = out
        .split_once(
            "#times
",
        )
        .unwrap_or((&out, ""));
    let mut timers = parse_timers(show, times);
    timers.sort_by(|a, b| a.unit.cmp(&b.unit));
    Ok(timers)
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UnitFile {
    pub path: String,
    pub content: String,
    /// Created by Jarvis, so it may be deleted from here.
    pub managed: bool,
}

async fn fragment_path(session: &Session, unit: &str) -> AppResult<String> {
    let out = session.run(format!("systemctl show -p FragmentPath --value {}", q(unit))).await?;
    let path = out.trim();
    if path.is_empty() {
        return Err(AppError::new(ErrorCode::NotFound, format!("{unit} has no unit file")));
    }
    Ok(path.to_string())
}

async fn write_unit(session: &Session, path: &str, content: &str) -> AppResult<()> {
    session.exec(Exec::new(format!("cat > {p} && chmod 644 {p}", p = q(path))).sudo().stdin(content)).await?.into_stdout()?;
    Ok(())
}

/// The system journal is only readable by root and members of a few groups.
pub async fn journal_needs_sudo(session: &Session) -> AppResult<bool> {
    if session.is_root() {
        return Ok(false);
    }
    let member = session.exec(Exec::new("id -nG | tr ' ' '\\n' | grep -qxE 'adm|systemd-journal|wheel'")).await?.success();
    Ok(!member)
}

// ---------------------------------------------------------------- commands

#[tauri::command]
#[specta::specta]
pub async fn services_list(state: State<'_, AppState>) -> AppResult<Vec<ServiceUnit>> {
    let session = state.session()?;
    list_services(&session).await
}

/// Start / stop / restart / reload / enable / disable any unit (service or timer).
#[tauri::command]
#[specta::specta]
pub async fn unit_action(state: State<'_, AppState>, unit: String, action: UnitAction) -> AppResult<()> {
    let session = state.session()?;
    validate::unit(&unit)?;
    session.exec(Exec::new(format!("systemctl {} {}", action.verb(), q(&unit))).sudo().secs(120)).await?.into_stdout()?;
    Ok(())
}

/// `systemctl status` text (non-zero exit just means "not running").
#[tauri::command]
#[specta::specta]
pub async fn unit_status(state: State<'_, AppState>, unit: String) -> AppResult<String> {
    let session = state.session()?;
    validate::unit(&unit)?;
    let out = session.exec_auto(Exec::new(format!("systemctl status --no-pager -l -n 30 {} 2>&1", q(&unit)))).await?;
    Ok(out.stdout)
}

#[tauri::command]
#[specta::specta]
pub async fn unit_logs(state: State<'_, AppState>, unit: String, lines: u32) -> AppResult<String> {
    let session = state.session()?;
    validate::unit(&unit)?;
    let lines = lines.clamp(10, 10_000);
    session.run_auto(format!("journalctl -u {} -n {lines} --no-pager -o short-iso 2>&1", q(&unit))).await
}

/// Follow a unit's journal as a hidden streamed job.
#[tauri::command]
#[specta::specta]
pub async fn unit_logs_follow(app: AppHandle, state: State<'_, AppState>, unit: String) -> AppResult<String> {
    let session = state.session()?;
    validate::unit(&unit)?;
    let exec = Exec::new(format!("journalctl -u {} -f -n 200 --no-pager -o short-iso 2>&1", q(&unit)));
    let sudo = journal_needs_sudo(&session).await?;
    state.jobs.start(&app, session, JobMeta::hidden(format!("Journal · {unit}")), exec.sudo_if(sudo)).await
}

#[tauri::command]
#[specta::specta]
pub async fn unit_file(state: State<'_, AppState>, unit: String) -> AppResult<UnitFile> {
    let session = state.session()?;
    validate::unit(&unit)?;
    let path = fragment_path(&session, &unit).await?;
    let content = session.run_auto(format!("cat -- {}", q(&path))).await?;
    Ok(UnitFile { managed: content.starts_with(MANAGED_MARK), path, content })
}

/// Save an edited unit file and reload systemd. Vendor units are not edited in
/// place: the new content goes to `/etc/systemd/system`, which takes precedence.
#[tauri::command]
#[specta::specta]
pub async fn unit_file_save(state: State<'_, AppState>, unit: String, content: String) -> AppResult<()> {
    let session = state.session()?;
    validate::unit(&unit)?;
    let current = fragment_path(&session, &unit).await?;
    let target = if current.starts_with(UNIT_DIR) { current } else { format!("{UNIT_DIR}/{unit}") };
    write_unit(&session, &target, &content).await?;
    session.run_sudo("systemctl daemon-reload").await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn service_create(state: State<'_, AppState>, spec: ServiceSpec) -> AppResult<()> {
    let session = state.session()?;
    let content = render_service(&spec)?;
    let unit = format!("{}.service", spec.name);
    let path = format!("{UNIT_DIR}/{unit}");
    if session.exec(Exec::new(format!("[ -e {} ]", q(&path)))).await?.success() {
        return Err(AppError::new(ErrorCode::AlreadyExists, unit));
    }
    write_unit(&session, &path, &content).await?;
    let mut script = String::from("systemctl daemon-reload");
    match (spec.enable, spec.start) {
        (true, true) => script.push_str(&format!(" && systemctl enable --now {}", q(&unit))),
        (true, false) => script.push_str(&format!(" && systemctl enable {}", q(&unit))),
        (false, true) => script.push_str(&format!(" && systemctl start {}", q(&unit))),
        (false, false) => {}
    }
    session.exec(Exec::new(script).sudo().secs(120)).await?.into_stdout()?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn timer_create(state: State<'_, AppState>, spec: TimerSpec) -> AppResult<()> {
    let session = state.session()?;
    let (service, timer) = render_timer(&spec)?;
    let service_path = format!("{UNIT_DIR}/{}.service", spec.name);
    let timer_path = format!("{UNIT_DIR}/{}.timer", spec.name);
    let exists = session.exec(Exec::new(format!("[ -e {} ] || [ -e {} ]", q(&service_path), q(&timer_path)))).await?.success();
    if exists {
        return Err(AppError::new(ErrorCode::AlreadyExists, spec.name));
    }
    write_unit(&session, &service_path, &service).await?;
    write_unit(&session, &timer_path, &timer).await?;
    session
        .exec(
            Exec::new(format!("systemctl daemon-reload && systemctl enable --now {}", q(&format!("{}.timer", spec.name)))).sudo().secs(120),
        )
        .await?
        .into_stdout()?;
    Ok(())
}

/// Delete a unit Jarvis created (a timer takes its service with it).
#[tauri::command]
#[specta::specta]
pub async fn unit_delete(state: State<'_, AppState>, unit: String) -> AppResult<()> {
    let session = state.session()?;
    validate::unit(&unit)?;
    let mut units = vec![unit.clone()];
    if let Some(base) = unit.strip_suffix(".timer") {
        units.push(format!("{base}.service"));
    }
    let mut script = String::new();
    for unit in &units {
        let path = q(&format!("{UNIT_DIR}/{unit}"));
        // Refuse anything that does not carry the Jarvis marker.
        script.push_str(&format!(
            "if [ -e {path} ]; then head -n 1 {path} | grep -qxF {mark} || {{ echo 'Not created by Jarvis' >&2; exit 9; }}; fi\n",
            mark = q(MANAGED_MARK)
        ));
    }
    for unit in &units {
        let u = q(unit);
        script.push_str(&format!("systemctl disable --now {u} >/dev/null 2>&1; rm -f {}\n", q(&format!("{UNIT_DIR}/{unit}"))));
    }
    script.push_str("systemctl daemon-reload; systemctl reset-failed >/dev/null 2>&1; true");
    let out = session.exec(Exec::new(script).sudo().secs(120)).await?;
    if out.code == 9 {
        return Err(AppError::invalid("Only units created by Jarvis can be deleted here"));
    }
    out.into_stdout()?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn timers_list(state: State<'_, AppState>) -> AppResult<Vec<TimerUnit>> {
    let session = state.session()?;
    list_timers(&session).await
}

/// Run a timer's service right now.
#[tauri::command]
#[specta::specta]
pub async fn timer_run_now(state: State<'_, AppState>, timer: String) -> AppResult<()> {
    let session = state.session()?;
    validate::unit(&timer)?;
    let service = session.run(format!("systemctl show -p Unit --value {}", q(&timer))).await?;
    let service = service.trim();
    if service.is_empty() {
        return Err(AppError::new(ErrorCode::NotFound, "The timer does not activate a service"));
    }
    session.exec(Exec::new(format!("systemctl start --no-block {}", q(service))).sudo()).await?.into_stdout()?;
    Ok(())
}

/// `systemctl status` plus `systemctl cat` for a timer and its service.
#[tauri::command]
#[specta::specta]
pub async fn timer_inspect(state: State<'_, AppState>, timer: String) -> AppResult<String> {
    let session = state.session()?;
    validate::unit(&timer)?;
    let t = q(&timer);
    let out = session
        .exec_auto(Exec::new(format!(
            "systemctl status --no-pager -l -n 10 {t} 2>&1; echo; echo '──── unit files ────'; \
             systemctl cat --no-pager {t} 2>&1; s=$(systemctl show -p Unit --value {t}); \
             [ -n \"$s\" ] && {{ echo; systemctl cat --no-pager \"$s\" 2>&1; }}; true"
        )))
        .await?;
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_units_and_unit_files() {
        let units = "\
  cron.service          loaded    active   running Regular background program processing daemon
● broken.service        loaded    failed   failed  A broken thing
  getty@tty1.service    loaded    active   running Getty on tty1
  dbus.socket           loaded    active   running D-Bus socket
  ghost.service         not-found inactive dead    ghost.service
";
        let files = "\
cron.service        enabled  enabled
broken.service      disabled enabled
getty@.service      enabled  enabled
rescue.service      static   -
";
        let s = parse_services(units, files);
        let names: Vec<&str> = s.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["broken.service", "cron.service", "getty@tty1.service", "ghost.service", "rescue.service"]);
        let cron = &s[1];
        assert_eq!((cron.active.as_str(), cron.sub.as_str(), cron.enabled.as_str()), ("active", "running", "enabled"));
        assert_eq!(cron.description, "Regular background program processing daemon");
        assert_eq!(s[0].active, "failed");
        assert_eq!(s[3].load, "not-found");
        // Known only from unit files: not loaded, but listed with its enablement.
        assert_eq!((s[4].active.as_str(), s[4].enabled.as_str()), ("inactive", "static"));
    }

    fn spec() -> ServiceSpec {
        ServiceSpec {
            name: "myapp".into(),
            description: "My app".into(),
            exec_start: "/usr/bin/node /srv/app/server.js".into(),
            user: "deploy".into(),
            restart: RestartPolicy::OnFailure,
            working_dir: "/srv/app".into(),
            environment: vec![("NODE_ENV".into(), "production".into()), ("MSG".into(), "say \"hi\"".into())],
            enable: true,
            start: true,
        }
    }

    #[test]
    fn renders_service_unit() {
        let unit = render_service(&spec()).unwrap();
        assert_eq!(
            unit,
            "# Managed by Jarvis Server Manager\n[Unit]\nDescription=My app\nAfter=network.target\n\n[Service]\nType=simple\n\
             ExecStart=/usr/bin/node /srv/app/server.js\nUser=deploy\nWorkingDirectory=/srv/app\n\
             Environment=\"NODE_ENV=production\"\nEnvironment=\"MSG=say \\\"hi\\\"\"\nRestart=on-failure\nRestartSec=5\n\n\
             [Install]\nWantedBy=multi-user.target\n"
        );
    }

    #[test]
    fn service_spec_validation() {
        let mut s = spec();
        s.name = "bad name".into();
        assert!(render_service(&s).is_err());
        let mut s = spec();
        s.exec_start = "a\nExecStartPost=/bin/evil".into();
        assert!(render_service(&s).is_err());
        let mut s = spec();
        s.exec_start = "  ".into();
        assert!(render_service(&s).is_err());
        let mut s = spec();
        s.environment = vec![("BAD KEY".into(), "x".into())];
        assert!(render_service(&s).is_err());
        let mut s = spec();
        s.working_dir = "relative".into();
        assert!(render_service(&s).is_err());
    }

    #[test]
    fn renders_timer_pair() {
        let (service, timer) = render_timer(&TimerSpec {
            name: "nightly".into(),
            description: String::new(),
            on_calendar: "*-*-* 03:00:00".into(),
            command: "/usr/local/bin/cleanup.sh".into(),
            user: String::new(),
            persistent: true,
        })
        .unwrap();
        assert!(service.starts_with(MANAGED_MARK));
        assert!(service.contains("Type=oneshot\nExecStart=/usr/local/bin/cleanup.sh\n"));
        assert!(!service.contains("User="));
        assert!(timer.contains("OnCalendar=*-*-* 03:00:00\nPersistent=true\n"));
        assert!(timer.contains("WantedBy=timers.target"));
    }

    #[test]
    fn parses_timer_blocks() {
        let show = "Id=apt-daily.timer
Description=Daily apt download activities
Unit=apt-daily.service
UnitFileState=enabled
ActiveState=active

Id=never.timer
Description=Never ran
Unit=never.service
UnitFileState=disabled
ActiveState=inactive

Id=ssh.service
Description=not a timer
";
        let times = r#"[{"next":1759525200000000,"left":1,"last":1759438800123456,"passed":2,"unit":"apt-daily.timer","activates":"apt-daily.service"},
            {"next":null,"left":null,"last":0,"passed":0,"unit":"never.timer","activates":"never.service"}]"#;
        let t = parse_timers(show, times);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].unit, "apt-daily.timer");
        assert_eq!(t[0].activates, "apt-daily.service");
        assert_eq!((t[0].next, t[0].last), (Some(1_759_525_200), Some(1_759_438_800)));
        assert_eq!((t[1].next, t[1].last), (None, None));
        assert_eq!(t[1].enabled, "disabled");
        // Old systemd without JSON output: units are listed, times unknown.
        let legacy = parse_timers(show, "");
        assert_eq!(legacy.len(), 2);
        assert_eq!(legacy[0].next, None);
    }
}
