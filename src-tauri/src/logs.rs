//! Log viewer (journal and files, followed live), login sessions, and
//! access-log analysis.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::docker::{in_target, ExecTarget};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::JobMeta;
use crate::shell::{q, validate};
use crate::ssh::session::Exec;
use crate::state::AppState;
use crate::systemd::journal_needs_sudo;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogSourceInfo {
    pub id: String,
    pub label: String,
    /// Path of the file, or empty for the journal.
    pub path: String,
    pub available: bool,
}

/// Built-in sources; each lists the usual locations across distributions.
const FILE_SOURCES: [(&str, &str, &[&str]); 4] = [
    ("syslog", "System log", &["/var/log/syslog", "/var/log/messages"]),
    ("auth", "Authentication log", &["/var/log/auth.log", "/var/log/secure"]),
    ("nginx-access", "Nginx access log", &["/var/log/nginx/access.log"]),
    ("nginx-error", "Nginx error log", &["/var/log/nginx/error.log"]),
];

#[tauri::command]
#[specta::specta]
pub async fn logs_sources(state: State<'_, AppState>) -> AppResult<Vec<LogSourceInfo>> {
    let session = state.session()?;
    let mut script = String::from("command -v journalctl >/dev/null 2>&1 && echo journal:1 || echo journal:0\n");
    for (id, _, paths) in FILE_SOURCES {
        let tests: Vec<String> = paths.iter().map(|p| format!("[ -e {p} ] && echo {id}:{p}")).collect();
        script.push_str(&format!("{{ {}; }} || echo {id}:\n", tests.join(" || ")));
    }
    let out = session.run(script).await?;
    let found = |id: &str| out.lines().find_map(|l| l.strip_prefix(&format!("{id}:"))).unwrap_or("").to_string();
    let mut sources = vec![LogSourceInfo {
        id: "journal".into(),
        label: "Systemd journal".into(),
        path: String::new(),
        available: found("journal") == "1",
    }];
    for (id, label, _) in FILE_SOURCES {
        let path = found(id);
        sources.push(LogSourceInfo { id: id.to_string(), label: label.to_string(), available: !path.is_empty(), path });
    }
    Ok(sources)
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LogQuery {
    /// The journal, optionally limited to one unit and/or a minimum priority.
    Journal {
        unit: String,
        priority: String,
    },
    File {
        path: String,
    },
}

const PRIORITIES: [&str; 8] = ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"];

pub fn follow_command(query: &LogQuery, lines: u32) -> AppResult<String> {
    let lines = lines.clamp(10, 50_000);
    match query {
        LogQuery::Journal { unit, priority } => {
            let mut cmd = format!("journalctl -f -n {lines} --no-pager -o short-iso");
            if !unit.trim().is_empty() {
                cmd.push_str(&format!(" -u {}", q(validate::unit(unit.trim())?)));
            }
            if !priority.trim().is_empty() {
                cmd.push_str(&format!(" -p {}", validate::one_of("priority", priority.trim(), &PRIORITIES)?));
            }
            Ok(cmd)
        }
        // -F keeps following across log rotation.
        LogQuery::File { path } => Ok(format!("tail -n {lines} -F {}", q(validate::abs_path(path.trim())?))),
    }
}

/// Follow a log as a hidden streamed job. Uses sudo when the user cannot read it.
#[tauri::command]
#[specta::specta]
pub async fn logs_follow(app: AppHandle, state: State<'_, AppState>, query: LogQuery, lines: u32) -> AppResult<String> {
    let session = state.session()?;
    let command = follow_command(&query, lines)?;
    let sudo = match &query {
        LogQuery::Journal { .. } => {
            if !session.has_command("journalctl").await? {
                return Err(AppError::unsupported("This server has no systemd journal"));
            }
            journal_needs_sudo(&session).await?
        }
        LogQuery::File { path } => {
            let p = q(path.trim());
            let probe = session.exec(Exec::new(format!("[ -e {p} ] || exit 5; [ -r {p} ] || exit 4"))).await?;
            match probe.code {
                0 => false,
                4 => true,
                5 => {
                    // The directory may be unreadable: only root can tell whether the file exists.
                    let as_root = session.exec(Exec::new(format!("[ -e {p} ]")).sudo()).await?;
                    if !as_root.success() {
                        return Err(AppError::new(ErrorCode::NotFound, path.trim()));
                    }
                    true
                }
                _ => return Err(AppError::new(ErrorCode::CommandFailed, probe.failure_text())),
            }
        }
    };
    state.jobs.start(&app, session, JobMeta::hidden("Log follow"), Exec::new(format!("{command} 2>&1")).sudo_if(sudo)).await
}

// ---------------------------------------------------------------- sessions

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LoginSession {
    pub user: String,
    pub tty: String,
    pub from: String,
    pub login: String,
    /// A terminal opened from this Jarvis window.
    pub own: bool,
}

/// Parse `who`: `user tty YYYY-MM-DD HH:MM (from)`.
pub fn parse_who(text: &str, own_ttys: &[String]) -> Vec<LoginSession> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 3 {
                return None;
            }
            let from = f
                .iter()
                .find(|w| w.starts_with('(') && w.ends_with(')'))
                .map(|w| w.trim_matches(['(', ')']).to_string())
                .unwrap_or_default();
            let login = f[2..].iter().filter(|w| !w.starts_with('(')).copied().collect::<Vec<_>>().join(" ");
            Some(LoginSession { user: f[0].to_string(), tty: f[1].to_string(), own: own_ttys.iter().any(|t| t == f[1]), from, login })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LoginRecord {
    pub user: String,
    pub tty: String,
    pub from: String,
    /// Login time and duration as printed by `last`.
    pub when: String,
}

/// Parse `last` / `lastb` output.
pub fn parse_last(text: &str) -> Vec<LoginRecord> {
    text.lines()
        .filter_map(|line| {
            if line.trim().is_empty() || line.starts_with("wtmp") || line.starts_with("btmp") {
                return None;
            }
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 4 || f[0] == "reboot" {
                return None;
            }
            // The third column is the origin unless it already is the weekday.
            const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
            let (from, when_start) = if DAYS.contains(&f[2]) { ("", 2) } else { (f[2], 3) };
            Some(LoginRecord { user: f[0].to_string(), tty: f[1].to_string(), from: from.to_string(), when: f[when_start..].join(" ") })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Sessions {
    pub current: Vec<LoginSession>,
    pub recent: Vec<LoginRecord>,
}

#[tauri::command]
#[specta::specta]
pub async fn sessions_list(state: State<'_, AppState>) -> AppResult<Sessions> {
    let session = state.session()?;
    let pids = state.terminals.shell_pids();
    let own_script = if pids.is_empty() {
        "true".to_string()
    } else {
        let list: Vec<String> = pids.iter().map(u32::to_string).collect();
        format!("ps -o tty= -p {} 2>/dev/null", list.join(","))
    };
    let out = session.run(format!("who 2>/dev/null; echo '#own'; {own_script}; echo '#last'; last -n 50 2>/dev/null; true")).await?;
    let (who, rest) = out.split_once("#own\n").unwrap_or((&out, ""));
    let (own, last) = rest.split_once("#last\n").unwrap_or((rest, ""));
    let own_ttys: Vec<String> = own.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    Ok(Sessions { current: parse_who(who, &own_ttys), recent: parse_last(last) })
}

/// Failed login attempts (`lastb`, root only).
#[tauri::command]
#[specta::specta]
pub async fn sessions_failed(state: State<'_, AppState>) -> AppResult<Vec<LoginRecord>> {
    let session = state.session()?;
    let out = session.exec(Exec::new("lastb -n 100 2>/dev/null; true").sudo()).await?;
    Ok(parse_last(&out.stdout))
}

/// End another user's login session by killing everything on its terminal.
#[tauri::command]
#[specta::specta]
pub async fn session_kick(state: State<'_, AppState>, tty: String) -> AppResult<()> {
    let session = state.session()?;
    let ok = !tty.is_empty() && tty.len() <= 32 && tty.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'/') && !tty.contains("..");
    if !ok {
        return Err(AppError::invalid("Invalid terminal name"));
    }
    session.exec(Exec::new(format!("pkill -KILL -t {}", q(&tty))).sudo()).await?.into_stdout()?;
    Ok(())
}

// ---------------------------------------------------------------- access-log analysis

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSource {
    pub target: ExecTarget,
    /// Access-log path; empty reads `docker logs` of the container instead.
    pub log_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Count {
    pub key: String,
    pub count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogAnalysis {
    pub total: u64,
    pub statuses: Vec<Count>,
    pub methods: Vec<Count>,
    pub top_ips: Vec<Count>,
    pub top_paths: Vec<Count>,
    pub top_agents: Vec<Count>,
    /// Requests per hour in chronological order; key is `YYYY-MM-DD HH:00`.
    pub per_hour: Vec<Count>,
}

/// Aggregates a common/combined access log read from stdin. The heavy
/// counting happens in awk on the server; only the top entries come back.
/// Output: one `TYPE<TAB>key<TAB>count` line per aggregate.
const ANALYSIS_AWK: &str = r#"awk '
{
  total++
  ip[$1]++
  n = split($0, part, "\"")
  if (n >= 3) {
    split(part[2], req, " ")
    if (req[1] != "") method[req[1]]++
    if (req[2] != "") path[req[2]]++
    split(part[3], rest, " ")
    if (rest[1] ~ /^[0-9][0-9][0-9]$/) status[rest[1]]++
  }
  if (n >= 6 && part[6] != "") agent[part[6]]++
  if (match($0, /\[[0-9][0-9]\/[A-Za-z][A-Za-z][A-Za-z]\/[0-9][0-9][0-9][0-9]:[0-9][0-9]/)) hour[substr($0, RSTART + 1, RLENGTH - 1)]++
}
END {
  printf "T\ttotal\t%d\n", total
  for (k in status) printf "S\t%s\t%d\n", k, status[k]
  for (k in method) printf "M\t%s\t%d\n", k, method[k]
  for (k in hour) printf "H\t%s\t%d\n", k, hour[k]
  for (k in ip) printf "I\t%s\t%d\n", k, ip[k]
  for (k in path) printf "P\t%s\t%d\n", k, path[k]
  for (k in agent) printf "U\t%s\t%d\n", k, agent[k]
}' | sort -t "$(printf '\t')" -k1,1 -k3,3nr | awk -F '\t' '{ seen[$1]++; if ($1 == "T" || $1 == "S" || $1 == "M" || $1 == "H" || seen[$1] <= 20) print }'"#;

/// The aggregation pipeline (reads an access log on stdin).
pub fn analysis_pipeline() -> &'static str {
    ANALYSIS_AWK
}

/// `03/Oct/2026:20` → `2026-10-03 20:00`.
fn hour_key(raw: &str) -> Option<String> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (date, hour) = raw.split_once(':')?;
    let mut parts = date.split('/');
    let (day, month, year) = (parts.next()?, parts.next()?, parts.next()?);
    let month = MONTHS.iter().position(|m| *m == month)? + 1;
    Some(format!("{year}-{month:02}-{day} {hour}:00"))
}

pub fn parse_analysis(output: &str) -> LogAnalysis {
    let mut analysis = LogAnalysis::default();
    for line in output.lines() {
        let mut fields = line.splitn(2, '\t');
        let (Some(kind), Some(rest)) = (fields.next(), fields.next()) else {
            continue;
        };
        // The key itself may contain tabs (user agents); the count is the last field.
        let Some((key, count)) = rest.rsplit_once('\t') else {
            continue;
        };
        let Ok(count) = count.trim().parse::<u64>() else {
            continue;
        };
        let entry = Count { key: key.to_string(), count };
        match kind {
            "T" => analysis.total = count,
            "S" => analysis.statuses.push(entry),
            "M" => analysis.methods.push(entry),
            "I" => analysis.top_ips.push(entry),
            "P" => analysis.top_paths.push(entry),
            "U" => analysis.top_agents.push(entry),
            "H" => {
                if let Some(key) = hour_key(key) {
                    analysis.per_hour.push(Count { key, count });
                }
            }
            _ => {}
        }
    }
    analysis.statuses.sort_by(|a, b| a.key.cmp(&b.key));
    analysis.per_hour.sort_by(|a, b| a.key.cmp(&b.key));
    analysis
}

#[tauri::command]
#[specta::specta]
pub async fn log_analyze(state: State<'_, AppState>, source: AnalysisSource, lines: u32) -> AppResult<LogAnalysis> {
    let session = state.session()?;
    let lines = lines.clamp(1_000, 1_000_000);
    let path = source.log_path.trim();
    let exec = if path.is_empty() {
        // No file: the web server logs to the container's stdout.
        let ExecTarget::Container { container } = &source.target else {
            return Err(AppError::invalid("Enter the path of the access log"));
        };
        let script = format!("docker logs --tail {lines} {} 2>&1 | {ANALYSIS_AWK}", q(validate::name("container", container)?));
        crate::docker::exec(&session, script).await?
    } else {
        let p = q(validate::abs_path(path)?);
        // Exit 5 / 4 are reported by the reader before the pipeline output matters.
        let script = format!("[ -e {p} ] || exit 5; [ -r {p} ] || exit 4; tail -n {lines} {p} | {ANALYSIS_AWK}");
        in_target(&session, &source.target, &script, false).await?
    };
    let exec = exec.secs(180);
    let mut out = session.exec(exec.clone()).await?;
    if out.code == 4 && source.target.is_host() && !session.is_root() {
        out = session.exec(exec.sudo()).await?;
    }
    match out.code {
        0 => Ok(parse_analysis(&out.stdout)),
        5 => Err(AppError::new(ErrorCode::NotFound, path)),
        4 => Err(AppError::new(ErrorCode::PermissionDenied, path)),
        _ => Err(out.into_stdout().unwrap_err()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_commands() {
        assert_eq!(
            follow_command(&LogQuery::Journal { unit: String::new(), priority: String::new() }, 200).unwrap(),
            "journalctl -f -n 200 --no-pager -o short-iso"
        );
        assert_eq!(
            follow_command(&LogQuery::Journal { unit: "nginx.service".into(), priority: "err".into() }, 5).unwrap(),
            "journalctl -f -n 10 --no-pager -o short-iso -u nginx.service -p err"
        );
        assert_eq!(
            follow_command(&LogQuery::File { path: "/var/log/my app.log".into() }, 100).unwrap(),
            "tail -n 100 -F '/var/log/my app.log'"
        );
        assert!(follow_command(&LogQuery::Journal { unit: "x; reboot".into(), priority: String::new() }, 100).is_err());
        assert!(follow_command(&LogQuery::Journal { unit: String::new(), priority: "loud".into() }, 100).is_err());
        assert!(follow_command(&LogQuery::File { path: "relative.log".into() }, 100).is_err());
    }

    #[test]
    fn who_output() {
        let text = "admin    pts/0        2026-10-03 20:19 (172.18.0.1)\nroot     tty1         2026-10-01 08:00\ndeploy   pts/3        2026-10-03 21:00 (2001:db8::7)\n";
        let s = parse_who(text, &["pts/3".to_string()]);
        assert_eq!(s.len(), 3);
        assert_eq!(
            s[0],
            LoginSession {
                user: "admin".into(),
                tty: "pts/0".into(),
                from: "172.18.0.1".into(),
                login: "2026-10-03 20:19".into(),
                own: false
            }
        );
        assert_eq!((s[1].from.as_str(), s[1].login.as_str()), ("", "2026-10-01 08:00"));
        assert!(s[2].own);
        assert_eq!(s[2].from, "2001:db8::7");
    }

    #[test]
    fn last_output() {
        let text = "admin    pts/0        172.18.0.1       Sat Oct  3 20:19   still logged in\nroot     tty1                          Thu Oct  1 08:00 - 09:10  (01:10)\nreboot   system boot  6.6.87           Thu Oct  1 07:59   still running\n\nwtmp begins Thu Oct  1 07:59:00 2026\n";
        let r = parse_last(text);
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[0].user.as_str(), r[0].from.as_str(), r[0].when.as_str()),
            ("admin", "172.18.0.1", "Sat Oct 3 20:19 still logged in")
        );
        assert_eq!((r[1].from.as_str(), r[1].when.as_str()), ("", "Thu Oct 1 08:00 - 09:10 (01:10)"));
    }

    #[test]
    fn analysis_output() {
        let out = "H\t03/Oct/2026:21\t5\nH\t03/Oct/2026:09\t7\nH\t30/Sep/2026:23\t1\nI\t203.0.113.9\t9\nI\t198.51.100.4\t4\nM\tGET\t11\nM\tPOST\t2\nP\t/\t8\nP\t/api/items?page=2\t5\nS\t404\t3\nS\t200\t10\nT\ttotal\t13\nU\tMozilla/5.0 (X11; Linux)\t12\nU\tcurl/8.5.0\t1\n";
        let a = parse_analysis(out);
        assert_eq!(a.total, 13);
        assert_eq!(a.statuses, vec![Count { key: "200".into(), count: 10 }, Count { key: "404".into(), count: 3 }]);
        assert_eq!(a.methods[0].key, "GET");
        assert_eq!(a.top_ips[0], Count { key: "203.0.113.9".into(), count: 9 });
        assert_eq!(a.top_paths[1].key, "/api/items?page=2");
        assert_eq!(a.top_agents[0].key, "Mozilla/5.0 (X11; Linux)");
        let hours: Vec<&str> = a.per_hour.iter().map(|h| h.key.as_str()).collect();
        assert_eq!(hours, ["2026-09-30 23:00", "2026-10-03 09:00", "2026-10-03 21:00"]);
    }

    #[test]
    fn analysis_tolerates_garbage() {
        let a = parse_analysis("nonsense\nT\ttotal\tNaN\n\nS\t200\n");
        assert_eq!(a, LogAnalysis::default());
        assert_eq!(hour_key("03/Foo/2026:20"), None);
    }
}
