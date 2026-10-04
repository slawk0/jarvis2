//! Environment variables: the current environment (read-only) and persistent
//! variables in shell profile files, tracked by a marker comment so Jarvis
//! only ever edits lines it wrote itself.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::files;
use crate::shell::{q, validate, Cmd};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

/// Comment line placed directly above every variable Jarvis manages.
pub const MARK: &str = "# jarvis-managed";

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EnvFile {
    Bashrc,
    Profile,
    BashProfile,
    /// `/etc/environment` (system-wide, needs root).
    Environment,
}

impl EnvFile {
    pub const ALL: [EnvFile; 4] = [EnvFile::Bashrc, EnvFile::Profile, EnvFile::BashProfile, EnvFile::Environment];

    fn path(self, home: &str) -> String {
        let home = home.trim_end_matches('/');
        match self {
            EnvFile::Bashrc => format!("{home}/.bashrc"),
            EnvFile::Profile => format!("{home}/.profile"),
            EnvFile::BashProfile => format!("{home}/.bash_profile"),
            EnvFile::Environment => "/etc/environment".to_string(),
        }
    }

    fn system(self) -> bool {
        self == EnvFile::Environment
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ManagedVar {
    pub file: EnvFile,
    pub path: String,
    pub key: String,
    pub value: String,
}

pub fn parse_env(text: &str) -> Vec<EnvVar> {
    let mut vars: Vec<EnvVar> = Vec::new();
    for line in text.lines() {
        match line.split_once('=') {
            Some((key, value)) if validate::env_key(key).is_ok() => vars.push(EnvVar { key: key.to_string(), value: value.to_string() }),
            // A value with line breaks continues on the following lines.
            _ => {
                if let Some(last) = vars.last_mut() {
                    last.value.push('\n');
                    last.value.push_str(line);
                }
            }
        }
    }
    vars.sort_by(|a, b| a.key.cmp(&b.key));
    vars
}

fn render_line(file: EnvFile, key: &str, value: &str) -> AppResult<String> {
    validate::env_key(key)?;
    validate::single_line("value", value)?;
    if file.system() {
        // pam_env reads this file literally: no escapes, so quotes cannot be represented.
        if value.contains('"') {
            return Err(AppError::invalid("Values in /etc/environment cannot contain double quotes"));
        }
        Ok(format!("{key}=\"{value}\""))
    } else {
        Ok(format!("export {key}={}", single_quote(value)))
    }
}

fn single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Read back a line written by [`render_line`].
fn parse_line(file: EnvFile, line: &str) -> Option<(String, String)> {
    let line = line.trim();
    let body = if file.system() { line } else { line.strip_prefix("export ")? };
    let (key, raw) = body.split_once('=')?;
    validate::env_key(key).ok()?;
    let value = if file.system() {
        raw.strip_prefix('"')?.strip_suffix('"')?.to_string()
    } else {
        raw.strip_prefix('\'')?.strip_suffix('\'')?.replace("'\\''", "'")
    };
    Some((key.to_string(), value))
}

/// Variables Jarvis manages in a file: a marker line followed by the assignment.
pub fn managed_vars(file: EnvFile, content: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = content.lines().collect();
    lines.windows(2).filter(|pair| pair[0].trim() == MARK).filter_map(|pair| parse_line(file, pair[1])).collect()
}

/// Add or replace a managed variable; everything else in the file is kept as is.
pub fn upsert_var(file: EnvFile, content: &str, key: &str, value: &str) -> AppResult<String> {
    let line = render_line(file, key, value)?;
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let existing = (1..lines.len()).find(|&i| lines[i - 1].trim() == MARK && parse_line(file, &lines[i]).is_some_and(|(k, _)| k == key));
    match existing {
        Some(i) => lines[i] = line,
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(MARK.to_string());
            lines.push(line);
        }
    }
    Ok(lines.join("\n") + "\n")
}

/// Remove a managed variable (marker and assignment). Other lines are untouched.
pub fn remove_var(file: EnvFile, content: &str, key: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let is_target = lines[i].trim() == MARK && lines.get(i + 1).and_then(|next| parse_line(file, next)).is_some_and(|(k, _)| k == key);
        if is_target {
            i += 2;
            // Drop the blank separator line we added in front, if any.
            if out.last().is_some_and(|l| l.trim().is_empty()) && lines.get(i).is_none_or(|l| l.trim().is_empty()) {
                out.pop();
            }
            continue;
        }
        out.push(lines[i]);
        i += 1;
    }
    if out.is_empty() {
        String::new()
    } else {
        out.join("\n") + "\n"
    }
}

async fn read_file(session: &Session, path: &str, sudo: bool) -> AppResult<String> {
    let out = session.exec(Exec::new(format!("f={}; [ -e \"$f\" ] || exit 0; cat -- \"$f\"", q(path))).sudo_if(sudo)).await?;
    out.into_stdout()
}

// ---------------------------------------------------------------- commands

/// The environment a login shell of the connected user gets.
#[tauri::command]
#[specta::specta]
pub async fn env_host(state: State<'_, AppState>) -> AppResult<Vec<EnvVar>> {
    let session = state.session()?;
    let out = session
        // Without the locale Jarvis forces for its own commands.
        .run("env -u LC_ALL -u LANG \"${SHELL:-/bin/sh}\" -lc env 2>/dev/null || env")
        .await?;
    Ok(parse_env(&out))
}

/// Environment of a Docker container (from its configuration).
#[tauri::command]
#[specta::specta]
pub async fn env_container(state: State<'_, AppState>, container: String) -> AppResult<Vec<EnvVar>> {
    let session = state.session()?;
    validate::name("container", &container)?;
    let sudo = crate::docker::needs_sudo(&session).await?;
    let cmd = Cmd::new("docker").lit("inspect --format").arg("{{range .Config.Env}}{{println .}}{{end}}").arg(&container);
    let out = session.exec(Exec::new(cmd.build()).sudo_if(sudo)).await?.into_stdout()?;
    Ok(parse_env(&out))
}

/// Persistent variables managed by Jarvis across the profile files.
#[tauri::command]
#[specta::specta]
pub async fn env_managed(state: State<'_, AppState>) -> AppResult<Vec<ManagedVar>> {
    let session = state.session()?;
    let mut vars = Vec::new();
    for file in EnvFile::ALL {
        let path = file.path(&session.facts.home);
        // These files are world-readable in practice; no elevation for listing.
        let content = read_file(&session, &path, false).await.unwrap_or_default();
        for (key, value) in managed_vars(file, &content) {
            vars.push(ManagedVar { file, path: path.clone(), key, value });
        }
    }
    Ok(vars)
}

#[tauri::command]
#[specta::specta]
pub async fn env_set(state: State<'_, AppState>, file: EnvFile, key: String, value: String) -> AppResult<()> {
    let session = state.session()?;
    let path = file.path(&session.facts.home);
    let sudo = file.system() && !session.is_root();
    let content = read_file(&session, &path, sudo).await?;
    let next = upsert_var(file, &content, key.trim(), &value)?;
    if sudo {
        session.exec(Exec::new(format!("cat > {}", q(&path))).sudo().stdin(next)).await?.into_stdout()?;
    } else {
        files::write_text(&session, &path, &next).await?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn env_remove(state: State<'_, AppState>, file: EnvFile, key: String) -> AppResult<()> {
    let session = state.session()?;
    let path = file.path(&session.facts.home);
    let sudo = file.system() && !session.is_root();
    let content = read_file(&session, &path, sudo).await?;
    let next = remove_var(file, &content, key.trim());
    if next == content {
        return Ok(());
    }
    session.exec(Exec::new(format!("cat > {}", q(&path))).sudo_if(sudo).stdin(next)).await?.into_stdout()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env_output() {
        let vars = parse_env("PATH=/usr/bin:/bin\nHOME=/root\nMULTI=line one\nline two\nEMPTY=\nX=a=b\n");
        let map: Vec<(&str, &str)> = vars.iter().map(|v| (v.key.as_str(), v.value.as_str())).collect();
        assert_eq!(map, vec![("EMPTY", ""), ("HOME", "/root"), ("MULTI", "line one\nline two"), ("PATH", "/usr/bin:/bin"), ("X", "a=b")]);
    }

    #[test]
    fn upsert_adds_then_replaces_in_place() {
        let original = "# my bashrc\nalias ll='ls -l'\nexport OWN=1\n";
        let once = upsert_var(EnvFile::Bashrc, original, "API_URL", "https://x/y?a=1&b=2").unwrap();
        assert_eq!(once, "# my bashrc\nalias ll='ls -l'\nexport OWN=1\n\n# jarvis-managed\nexport API_URL='https://x/y?a=1&b=2'\n");
        let twice = upsert_var(EnvFile::Bashrc, &once, "API_URL", "it's new").unwrap();
        assert_eq!(twice.matches(MARK).count(), 1);
        assert!(twice.contains("export API_URL='it'\\''s new'\n"));
        assert!(twice.starts_with(original));
        assert_eq!(managed_vars(EnvFile::Bashrc, &twice), vec![("API_URL".to_string(), "it's new".to_string())]);
        // The user's own export is not considered managed.
        assert!(!managed_vars(EnvFile::Bashrc, &twice).iter().any(|(k, _)| k == "OWN"));
    }

    #[test]
    fn remove_restores_the_original_file() {
        let original = "# my bashrc\nexport OWN=1\n";
        let with = upsert_var(EnvFile::Bashrc, original, "A", "1").unwrap();
        let both = upsert_var(EnvFile::Bashrc, &with, "B", "2").unwrap();
        assert_eq!(managed_vars(EnvFile::Bashrc, &both).len(), 2);
        let without_a = remove_var(EnvFile::Bashrc, &both, "A");
        assert_eq!(managed_vars(EnvFile::Bashrc, &without_a), vec![("B".to_string(), "2".to_string())]);
        assert_eq!(remove_var(EnvFile::Bashrc, &with, "A"), original);
        assert_eq!(remove_var(EnvFile::Bashrc, original, "OWN"), original);
        assert_eq!(remove_var(EnvFile::Bashrc, &upsert_var(EnvFile::Bashrc, "", "A", "1").unwrap(), "A"), "");
    }

    #[test]
    fn etc_environment_format() {
        let content = upsert_var(EnvFile::Environment, "PATH=\"/usr/bin\"\n", "JAVA_HOME", "/opt/java 17").unwrap();
        assert_eq!(content, "PATH=\"/usr/bin\"\n\n# jarvis-managed\nJAVA_HOME=\"/opt/java 17\"\n");
        assert_eq!(managed_vars(EnvFile::Environment, &content), vec![("JAVA_HOME".to_string(), "/opt/java 17".to_string())]);
        assert!(upsert_var(EnvFile::Environment, "", "X", "say \"hi\"").is_err());
    }

    #[test]
    fn rejects_bad_keys_and_multiline_values() {
        assert!(upsert_var(EnvFile::Profile, "", "BAD KEY", "x").is_err());
        assert!(upsert_var(EnvFile::Profile, "", "1X", "x").is_err());
        assert!(upsert_var(EnvFile::Profile, "", "OK", "a\nb").is_err());
    }

    #[test]
    fn file_paths() {
        assert_eq!(EnvFile::Bashrc.path("/home/me/"), "/home/me/.bashrc");
        assert_eq!(EnvFile::Environment.path("/home/me"), "/etc/environment");
    }
}
