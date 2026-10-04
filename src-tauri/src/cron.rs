//! Crontab editing. The crontab is treated as text: only the job line being
//! changed is touched, every other line (variables, comments, `@reboot`
//! entries, blocks owned by other tools) is preserved exactly. Writes go
//! through `crontab -` on stdin, never through temp files.

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::shell::validate;
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CronJob {
    /// Zero-based line number in the crontab.
    pub line: u32,
    pub enabled: bool,
    pub schedule: String,
    pub command: String,
    /// Inside a block managed by Jarvis Backups (read-only here).
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Crontab {
    pub jobs: Vec<CronJob>,
    /// The full text, for the raw view.
    pub raw: String,
}

pub fn block_begin(id: &str) -> String {
    format!("# BEGIN JARVIS {id}")
}

pub fn block_end(id: &str) -> String {
    format!("# END JARVIS {id}")
}

/// Split a crontab job line into schedule and command.
fn split_job(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    let fields = if line.starts_with('@') { 1 } else { 5 };
    let mut rest = line;
    let mut end = 0;
    for _ in 0..fields {
        let trimmed = rest.trim_start();
        let offset = line.len() - trimmed.len();
        let len = trimmed.find(char::is_whitespace)?;
        end = offset + len;
        rest = &line[end..];
    }
    let schedule = line[..end].split_whitespace().collect::<Vec<_>>().join(" ");
    let command = line[end..].trim();
    if command.is_empty() || validate::cron_expr(&schedule).is_err() {
        return None;
    }
    Some((schedule, command.to_string()))
}

pub fn parse(text: &str) -> Vec<CronJob> {
    let mut jobs = Vec::new();
    let mut in_block = false;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with("# BEGIN JARVIS ") {
            in_block = true;
            continue;
        }
        if line.starts_with("# END JARVIS ") {
            in_block = false;
            continue;
        }
        let (enabled, body) = match line.strip_prefix('#') {
            Some(rest) => (false, rest.trim_start()),
            None => (true, line),
        };
        // `KEY=value` lines are environment settings, not jobs.
        if body.is_empty() || (enabled && is_assignment(body)) {
            continue;
        }
        if let Some((schedule, command)) = split_job(body) {
            // `@reboot` and friends are shown as-is but are regular jobs too.
            jobs.push(CronJob { line: index as u32, enabled, schedule, command, managed: in_block });
        }
    }
    jobs
}

fn is_assignment(line: &str) -> bool {
    line.split_once('=').is_some_and(|(key, _)| {
        let key = key.trim();
        !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

fn job_line(schedule: &str, command: &str) -> AppResult<String> {
    let schedule = validate::cron_expr(schedule.trim())?.split_whitespace().collect::<Vec<_>>().join(" ");
    let command = validate::single_line("command", command.trim())?;
    if command.is_empty() {
        return Err(AppError::invalid("Enter a command"));
    }
    Ok(format!("{schedule} {command}"))
}

fn join(lines: Vec<String>) -> String {
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

/// Apply `edit` to the job at `line`, after checking it is still the job the
/// user saw (`expected` schedule and command) and not inside a managed block.
fn edit_job(text: &str, line: u32, expected: (&str, &str), edit: impl FnOnce(&CronJob) -> AppResult<Option<String>>) -> AppResult<String> {
    let jobs = parse(text);
    let job = jobs
        .iter()
        .find(|j| j.line == line && j.schedule == expected.0 && j.command == expected.1)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "The crontab changed; refresh and try again"))?;
    if job.managed {
        return Err(AppError::invalid("This entry is managed by Jarvis Backups"));
    }
    let replacement = edit(job)?;
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    match replacement {
        Some(new_line) => lines[line as usize] = new_line,
        None => {
            lines.remove(line as usize);
        }
    }
    Ok(join(lines))
}

pub fn add_job(text: &str, schedule: &str, command: &str) -> AppResult<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.push(job_line(schedule, command)?);
    Ok(join(lines))
}

pub fn update_job(text: &str, line: u32, expected: (&str, &str), schedule: &str, command: &str) -> AppResult<String> {
    edit_job(text, line, expected, |job| {
        let body = job_line(schedule, command)?;
        Ok(Some(if job.enabled { body } else { format!("# {body}") }))
    })
}

pub fn set_enabled(text: &str, line: u32, expected: (&str, &str), enabled: bool) -> AppResult<String> {
    edit_job(text, line, expected, |job| {
        let body = format!("{} {}", job.schedule, job.command);
        Ok(Some(if enabled { body } else { format!("# {body}") }))
    })
}

pub fn delete_job(text: &str, line: u32, expected: (&str, &str)) -> AppResult<String> {
    edit_job(text, line, expected, |_| Ok(None))
}

/// Insert or replace the marker-delimited block `id` (used for backup schedules).
pub fn upsert_block(text: &str, id: &str, body: &[String]) -> String {
    let mut lines: Vec<String> = remove_block(text, id).lines().map(str::to_string).collect();
    lines.push(block_begin(id));
    lines.extend(body.iter().cloned());
    lines.push(block_end(id));
    join(lines)
}

pub fn remove_block(text: &str, id: &str) -> String {
    let (begin, end) = (block_begin(id), block_end(id));
    let mut inside = false;
    let lines: Vec<String> = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed == begin {
                inside = true;
                return false;
            }
            if trimmed == end {
                inside = false;
                return false;
            }
            !inside
        })
        .map(str::to_string)
        .collect();
    join(lines)
}

/// Lines of block `id`, if present.
pub fn block_lines(text: &str, id: &str) -> Option<Vec<String>> {
    let (begin, end) = (block_begin(id), block_end(id));
    let mut lines = text.lines().map(str::trim).skip_while(|l| *l != begin);
    lines.next()?;
    Some(lines.take_while(|l| *l != end).map(str::to_string).collect())
}

/// Read a crontab; "no crontab for user" is simply an empty one.
pub async fn read(session: &Session, sudo: bool) -> AppResult<String> {
    let out = session.exec(Exec::new("crontab -l").sudo_if(sudo)).await?;
    if out.success() {
        Ok(out.stdout)
    } else if out.stderr.to_lowercase().contains("no crontab") {
        Ok(String::new())
    } else {
        Err(out.into_stdout().unwrap_err())
    }
}

pub async fn write(session: &Session, text: &str, sudo: bool) -> AppResult<()> {
    session.exec(Exec::new("crontab -").sudo_if(sudo).stdin(text)).await?.into_stdout()?;
    Ok(())
}

// ---------------------------------------------------------------- commands

#[tauri::command]
#[specta::specta]
pub async fn cron_list(state: State<'_, AppState>) -> AppResult<Crontab> {
    let session = state.session()?;
    let raw = read(&session, false).await?;
    Ok(Crontab { jobs: parse(&raw), raw })
}

/// Root's crontab (read-only view).
#[tauri::command]
#[specta::specta]
pub async fn cron_root(state: State<'_, AppState>) -> AppResult<Crontab> {
    let session = state.session()?;
    let raw = read(&session, true).await?;
    Ok(Crontab { jobs: parse(&raw), raw })
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CronFile {
    pub path: String,
    pub content: String,
}

/// Files in `/etc/cron.d` (read-only view).
#[tauri::command]
#[specta::specta]
pub async fn cron_system_files(state: State<'_, AppState>) -> AppResult<Vec<CronFile>> {
    let session = state.session()?;
    const MARK: &str = "#jarvis-file:";
    let out = session
        .run_auto(format!("for f in /etc/cron.d/*; do [ -f \"$f\" ] || continue; echo \"{MARK}$f\"; cat \"$f\"; echo; done; true"))
        .await?;
    let mut files: Vec<CronFile> = Vec::new();
    for line in out.lines() {
        if let Some(path) = line.strip_prefix(MARK) {
            files.push(CronFile { path: path.to_string(), content: String::new() });
        } else if let Some(file) = files.last_mut() {
            file.content.push_str(line);
            file.content.push('\n');
        }
    }
    Ok(files)
}

#[tauri::command]
#[specta::specta]
pub async fn cron_add(state: State<'_, AppState>, schedule: String, command: String) -> AppResult<()> {
    let session = state.session()?;
    let text = read(&session, false).await?;
    write(&session, &add_job(&text, &schedule, &command)?, false).await
}

/// The job is identified by its line plus the schedule/command the UI saw.
#[tauri::command]
#[specta::specta]
pub async fn cron_update(state: State<'_, AppState>, job: CronJobRef, schedule: String, command: String) -> AppResult<()> {
    let session = state.session()?;
    let text = read(&session, false).await?;
    let next = update_job(&text, job.line, (&job.schedule, &job.command), &schedule, &command)?;
    write(&session, &next, false).await
}

#[tauri::command]
#[specta::specta]
pub async fn cron_set_enabled(state: State<'_, AppState>, job: CronJobRef, enabled: bool) -> AppResult<()> {
    let session = state.session()?;
    let text = read(&session, false).await?;
    let next = set_enabled(&text, job.line, (&job.schedule, &job.command), enabled)?;
    write(&session, &next, false).await
}

#[tauri::command]
#[specta::specta]
pub async fn cron_delete(state: State<'_, AppState>, job: CronJobRef) -> AppResult<()> {
    let session = state.session()?;
    let text = read(&session, false).await?;
    let next = delete_job(&text, job.line, (&job.schedule, &job.command))?;
    write(&session, &next, false).await
}

#[derive(Debug, Clone, serde::Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CronJobRef {
    pub line: u32,
    pub schedule: String,
    pub command: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAB: &str = "\
# m h dom mon dow command
MAILTO=ops@example.com
PATH=/usr/bin:/bin

*/5 * * * * /usr/local/bin/poll --fast
# 0 3 * * * /usr/local/bin/nightly.sh >> /var/log/nightly.log 2>&1
@reboot /usr/local/bin/on-boot
# just a note about * * things
# BEGIN JARVIS backup-1
0 2 * * * /usr/local/bin/jarvis-backup-1.sh
# END JARVIS backup-1
30  4   1 * *   tar czf /tmp/x.tgz /etc
";

    #[test]
    fn parses_jobs_and_ignores_everything_else() {
        let jobs = parse(TAB);
        let summary: Vec<(u32, bool, &str, &str, bool)> =
            jobs.iter().map(|j| (j.line, j.enabled, j.schedule.as_str(), j.command.as_str(), j.managed)).collect();
        assert_eq!(
            summary,
            vec![
                (4, true, "*/5 * * * *", "/usr/local/bin/poll --fast", false),
                (5, false, "0 3 * * *", "/usr/local/bin/nightly.sh >> /var/log/nightly.log 2>&1", false),
                (6, true, "@reboot", "/usr/local/bin/on-boot", false),
                (9, true, "0 2 * * *", "/usr/local/bin/jarvis-backup-1.sh", true),
                (11, true, "30 4 1 * *", "tar czf /tmp/x.tgz /etc", false),
            ]
        );
    }

    #[test]
    fn toggling_only_touches_that_line() {
        let off = set_enabled(TAB, 4, ("*/5 * * * *", "/usr/local/bin/poll --fast"), false).unwrap();
        let changed: Vec<(&str, &str)> = TAB.lines().zip(off.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(changed, vec![("*/5 * * * * /usr/local/bin/poll --fast", "# */5 * * * * /usr/local/bin/poll --fast")]);
        let on = set_enabled(&off, 4, ("*/5 * * * *", "/usr/local/bin/poll --fast"), true).unwrap();
        assert_eq!(on, TAB);
    }

    #[test]
    fn add_update_delete_preserve_other_lines() {
        let added = add_job(TAB, "0 0 * * 0", "echo weekly").unwrap();
        assert!(added.ends_with("0 0 * * 0 echo weekly\n"));
        assert!(added.starts_with(TAB));

        let updated =
            update_job(&added, 5, ("0 3 * * *", "/usr/local/bin/nightly.sh >> /var/log/nightly.log 2>&1"), "15 3 * * *", "echo hi")
                .unwrap();
        // A disabled job stays disabled when edited.
        assert_eq!(updated.lines().nth(5).unwrap(), "# 15 3 * * * echo hi");
        assert_eq!(updated.lines().count(), added.lines().count());

        let deleted = delete_job(&updated, 6, ("@reboot", "/usr/local/bin/on-boot")).unwrap();
        assert!(!deleted.contains("on-boot"));
        assert!(deleted.contains("MAILTO=ops@example.com"));
        assert!(deleted.contains("# just a note about * * things"));
        assert_eq!(deleted.lines().count(), updated.lines().count() - 1);
    }

    #[test]
    fn stale_or_managed_edits_are_refused() {
        let e = delete_job(TAB, 4, ("* * * * *", "something else")).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        let e = delete_job(TAB, 9, ("0 2 * * *", "/usr/local/bin/jarvis-backup-1.sh")).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidInput);
        assert!(add_job(TAB, "* * * *", "x").is_err());
        assert!(add_job(TAB, "* * * * *", "a\nb").is_err());
        assert!(add_job(TAB, "* * * * *", "  ").is_err());
    }

    #[test]
    fn blocks_are_upserted_and_removed_cleanly() {
        let body = vec!["0 5 * * * /usr/local/bin/jarvis-backup-2.sh".to_string()];
        let with = upsert_block(TAB, "backup-2", &body);
        assert_eq!(block_lines(&with, "backup-2").unwrap(), body);
        assert_eq!(block_lines(&with, "backup-1").unwrap().len(), 1);

        let replaced = upsert_block(&with, "backup-2", &["1 1 * * * x".to_string()]);
        assert_eq!(block_lines(&replaced, "backup-2").unwrap(), vec!["1 1 * * * x"]);
        assert_eq!(replaced.matches("BEGIN JARVIS backup-2").count(), 1);

        assert_eq!(remove_block(&replaced, "backup-2"), TAB);
        assert_eq!(block_lines(TAB, "backup-2"), None);
        assert_eq!(upsert_block("", "a", &body), format!("# BEGIN JARVIS a\n{}\n# END JARVIS a\n", body[0]));
    }

    #[test]
    fn empty_crontab() {
        assert!(parse("").is_empty());
        assert_eq!(add_job("", "@daily", "echo x").unwrap(), "@daily echo x\n");
    }
}
