//! Restic repositories: configuration, snapshots, browsing and restore.
//!
//! Repository passwords and cloud credentials come from the OS keyring and
//! reach restic through its environment, which is sent over the channel's
//! stdin (`eval "$(cat)"`): never on a command line, never in a file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::JobMeta;
use crate::sftp::transfer::{self, ConflictPolicy, TransferKind, TransferRequest};
use crate::shell::{q, validate};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;
use crate::store::profile_data::DataKey;
use crate::text::looks_binary;

const PREVIEW_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResticRepo {
    pub id: String,
    pub name: String,
    /// `local`, `s3`, `b2`, `sftp`, `rest` or `rclone`.
    pub kind: String,
    /// The repository string restic understands, e.g. `s3:https://host/bucket/path`.
    pub repository: String,
    /// Type-specific form fields, kept so the edit form can be refilled.
    pub fields: BTreeMap<String, String>,
    /// Names of extra environment variables (their values are secrets).
    pub env_names: Vec<String>,
    pub sudo: bool,
}

/// Secret values sent when saving a repository. `None` keeps what is stored.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResticSecrets {
    pub password: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub env: Vec<(String, String)>,
}

fn secret(id: &str, name: &str) -> String {
    format!("restic/{id}/{name}")
}

const KINDS: [&str; 6] = ["local", "s3", "b2", "sftp", "rest", "rclone"];

pub fn repos(state: &AppState, session: &Session) -> AppResult<Vec<ResticRepo>> {
    state.data.get_as(&session.profile.id, DataKey::ResticRepos)
}

pub fn repo(state: &AppState, session: &Session, id: &str) -> AppResult<ResticRepo> {
    repos(state, session)?.into_iter().find(|r| r.id == id).ok_or_else(|| AppError::new(ErrorCode::NotFound, "restic repository"))
}

/// Environment for restic as (name, value) pairs, read from the keyring.
pub fn environment(state: &AppState, session: &Session, repo: &ResticRepo) -> AppResult<Vec<(String, String)>> {
    let owner = &session.profile.id;
    let get = |name: &str| state.secrets.get(owner, &secret(&repo.id, name));
    let mut env = vec![
        ("RESTIC_REPOSITORY".to_string(), repo.repository.clone()),
        ("RESTIC_PASSWORD".to_string(), get("password")?.unwrap_or_default()),
    ];
    let access = get("access-key")?.unwrap_or_default();
    let secret_key = get("secret-key")?.unwrap_or_default();
    match repo.kind.as_str() {
        "s3" => {
            env.push(("AWS_ACCESS_KEY_ID".into(), access));
            env.push(("AWS_SECRET_ACCESS_KEY".into(), secret_key));
        }
        "b2" => {
            env.push(("B2_ACCOUNT_ID".into(), access));
            env.push(("B2_ACCOUNT_KEY".into(), secret_key));
        }
        "rest" if !access.is_empty() => {
            env.push(("RESTIC_REST_USERNAME".into(), access));
            env.push(("RESTIC_REST_PASSWORD".into(), secret_key));
        }
        _ => {}
    }
    for name in &repo.env_names {
        let value = get(&format!("env/{name}"))?.unwrap_or_default();
        env.push((validate::env_key(name)?.to_string(), value));
    }
    Ok(env)
}

/// `export NAME='value'` lines for `eval "$(cat)"`.
pub fn env_exports(env: &[(String, String)]) -> String {
    env.iter().map(|(name, value)| format!("export {name}={}\n", single_quote(value))).collect()
}

fn single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// An `Exec` running `restic <args>` with the repository environment on stdin.
pub fn restic_exec(state: &AppState, session: &Session, repo: &ResticRepo, args: &str) -> AppResult<Exec> {
    let env = environment(state, session, repo)?;
    Ok(Exec::new(format!("eval \"$(cat)\"; restic {args}")).stdin(env_exports(&env)).sudo_if(repo.sudo && !session.is_root()))
}

async fn run(state: &AppState, session: &Session, repo: &ResticRepo, args: &str, secs: u64) -> AppResult<String> {
    session.exec(restic_exec(state, session, repo, args)?.secs(secs)).await?.into_stdout()
}

async fn job(app: &AppHandle, state: &AppState, id: &str, title: &str, args: String) -> AppResult<String> {
    let session = state.session()?;
    let repo = repo(state, &session, id)?;
    let exec = restic_exec(state, &session, &repo, &format!("{args} 2>&1"))?;
    let meta = JobMeta::visible(format!("{title} · {}", repo.name), format!("restic {args}"));
    state.jobs.start(app, session, meta, exec).await
}

fn snapshot_id(id: &str) -> AppResult<&str> {
    if !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) || id == "latest" {
        Ok(id)
    } else {
        Err(AppError::invalid("Invalid snapshot id"))
    }
}

fn tag(value: &str) -> AppResult<&str> {
    validate::slug("tag", value)
}

// ---------------------------------------------------------------- repositories

#[tauri::command]
#[specta::specta]
pub fn restic_repos(state: State<'_, AppState>) -> AppResult<Vec<ResticRepo>> {
    let session = state.session()?;
    repos(&state, &session)
}

#[tauri::command]
#[specta::specta]
pub fn restic_repo_save(state: State<'_, AppState>, repo: ResticRepo, secrets: ResticSecrets) -> AppResult<ResticRepo> {
    let session = state.session()?;
    let mut repo = repo;
    if repo.name.trim().is_empty() {
        return Err(AppError::invalid("Enter a name"));
    }
    validate::one_of("repository type", &repo.kind, &KINDS)?;
    validate::single_line("repository", repo.repository.trim())?;
    if repo.repository.trim().is_empty() {
        return Err(AppError::invalid("The repository location is incomplete"));
    }
    repo.repository = repo.repository.trim().to_string();
    if repo.id.is_empty() {
        repo.id = uuid::Uuid::new_v4().simple().to_string();
    } else {
        validate::slug("repository id", &repo.id)?;
    }
    let owner = session.profile.id.clone();
    let set = |name: &str, value: &Option<String>| -> AppResult<()> {
        if let Some(value) = value {
            state.secrets.set(&owner, &secret(&repo.id, name), value)?;
        }
        Ok(())
    };
    set("password", &secrets.password)?;
    set("access-key", &secrets.access_key)?;
    set("secret-key", &secrets.secret_key)?;
    for (name, value) in &secrets.env {
        validate::env_key(name)?;
        state.secrets.set(&owner, &secret(&repo.id, &format!("env/{name}")), value)?;
    }
    // Forget values of variables that were removed from the list.
    let previous = repos(&state, &session)?.into_iter().find(|r| r.id == repo.id);
    for old in previous.iter().flat_map(|r| r.env_names.iter()) {
        if !repo.env_names.contains(old) {
            state.secrets.delete(&owner, &secret(&repo.id, &format!("env/{old}")))?;
        }
    }
    let saved = repo.clone();
    state.data.update(&owner, DataKey::ResticRepos, |all: &mut Vec<ResticRepo>| match all.iter_mut().find(|r| r.id == repo.id) {
        Some(existing) => *existing = repo,
        None => all.push(repo),
    })?;
    Ok(saved)
}

/// Remove a repository's saved configuration and secrets. The data is not touched.
#[tauri::command]
#[specta::specta]
pub fn restic_repo_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let session = state.session()?;
    let owner = session.profile.id.clone();
    validate::slug("repository id", &id)?;
    state.secrets.delete_prefixed(&owner, &format!("restic/{id}/"))?;
    state.data.update(&owner, DataKey::ResticRepos, |all: &mut Vec<ResticRepo>| all.retain(|r| r.id != id))
}

/// Remotes configured for rclone on the server.
#[tauri::command]
#[specta::specta]
pub async fn restic_rclone_remotes(state: State<'_, AppState>, sudo: bool) -> AppResult<Vec<String>> {
    let session = state.session()?;
    let out = session.exec(Exec::new("rclone listremotes 2>/dev/null").sudo_if(sudo && !session.is_root())).await?;
    if out.code == 127 {
        return Err(AppError::new(ErrorCode::DependencyMissing, "rclone"));
    }
    Ok(out.stdout.lines().map(|l| l.trim().trim_end_matches(':').to_string()).filter(|l| !l.is_empty()).collect())
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    pub initialized: bool,
    pub version: String,
    /// Why the repository could not be opened, when it exists but access failed.
    pub problem: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn restic_status(state: State<'_, AppState>, id: String) -> AppResult<RepoStatus> {
    let session = state.session()?;
    let repo = repo(&state, &session, &id)?;
    let version = session.exec(Exec::new("restic version 2>/dev/null")).await?.stdout.split_whitespace().nth(1).unwrap_or("").to_string();
    let out = session.exec(restic_exec(&state, &session, &repo, "cat config --no-lock 2>&1")?.secs(60)).await?;
    let text = out.stdout.to_lowercase();
    // A missing repository is the normal "not initialised yet" state; anything else is a problem.
    let missing =
        text.contains("is there a repository at") || text.contains("does not exist") || text.contains("unable to open config file");
    Ok(RepoStatus { initialized: out.success(), version, problem: (!out.success() && !missing).then(|| out.stdout.trim().to_string()) })
}

#[tauri::command]
#[specta::specta]
pub async fn restic_init(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<String> {
    job(&app, &state, &id, "Initialise repository", "init".into()).await
}

#[tauri::command]
#[specta::specta]
pub async fn restic_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    paths: Vec<String>,
    tags: Vec<String>,
    excludes: Vec<String>,
) -> AppResult<String> {
    if paths.is_empty() {
        return Err(AppError::invalid("Choose at least one path to back up"));
    }
    let mut args = String::from("backup");
    for path in &paths {
        args.push_str(&format!(" {}", q(validate::abs_path(path.trim())?)));
    }
    for t in tags.iter().filter(|t| !t.trim().is_empty()) {
        args.push_str(&format!(" --tag {}", q(tag(t.trim())?)));
    }
    for e in excludes.iter().filter(|e| !e.trim().is_empty()) {
        args.push_str(&format!(" --exclude {}", q(validate::single_line("exclude pattern", e.trim())?)));
    }
    job(&app, &state, &id, "Backup", args).await
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ResticMaintenance {
    Check,
    Unlock,
    Prune,
}

#[tauri::command]
#[specta::specta]
pub async fn restic_maintenance(app: AppHandle, state: State<'_, AppState>, id: String, action: ResticMaintenance) -> AppResult<String> {
    let (title, args) = match action {
        ResticMaintenance::Check => ("Integrity check", "check"),
        ResticMaintenance::Unlock => ("Unlock", "unlock"),
        ResticMaintenance::Prune => ("Prune", "prune"),
    };
    job(&app, &state, &id, title, args.into()).await
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RepoStats {
    pub total_size: u64,
    pub total_file_count: u64,
    pub snapshots_count: u64,
}

pub fn parse_stats(json: &str) -> AppResult<RepoStats> {
    let v: serde_json::Value = serde_json::from_str(json.trim())?;
    let n = |key: &str| v.get(key).and_then(|x| x.as_u64()).unwrap_or(0);
    Ok(RepoStats { total_size: n("total_size"), total_file_count: n("total_file_count"), snapshots_count: n("snapshots_count") })
}

#[tauri::command]
#[specta::specta]
pub async fn restic_stats(state: State<'_, AppState>, id: String) -> AppResult<RepoStats> {
    let session = state.session()?;
    let repo = repo(&state, &session, &id)?;
    parse_stats(&run(&state, &session, &repo, "stats --json --mode raw-data", 300).await?)
}

// ---------------------------------------------------------------- snapshots

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub short_id: String,
    pub time: String,
    pub hostname: String,
    pub paths: Vec<String>,
    pub tags: Vec<String>,
}

pub fn parse_snapshots(json: &str) -> AppResult<Vec<Snapshot>> {
    #[derive(Deserialize)]
    struct Raw {
        id: String,
        #[serde(default)]
        short_id: String,
        #[serde(default)]
        time: String,
        #[serde(default)]
        hostname: String,
        #[serde(default)]
        paths: Vec<String>,
        #[serde(default)]
        tags: Option<Vec<String>>,
    }
    let text = json.trim();
    if text.is_empty() || text == "null" {
        return Ok(Vec::new());
    }
    let raw: Vec<Raw> = serde_json::from_str(text)?;
    let mut snapshots: Vec<Snapshot> = raw
        .into_iter()
        .map(|s| Snapshot {
            short_id: if s.short_id.is_empty() { s.id.chars().take(8).collect() } else { s.short_id },
            id: s.id,
            time: s.time,
            hostname: s.hostname,
            paths: s.paths,
            tags: s.tags.unwrap_or_default(),
        })
        .collect();
    snapshots.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(snapshots)
}

#[tauri::command]
#[specta::specta]
pub async fn restic_snapshots(state: State<'_, AppState>, id: String) -> AppResult<Vec<Snapshot>> {
    let session = state.session()?;
    let repo = repo(&state, &session, &id)?;
    parse_snapshots(&run(&state, &session, &repo, "snapshots --json", 120).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn restic_restore(app: AppHandle, state: State<'_, AppState>, id: String, snapshot: String, target: String) -> AppResult<String> {
    let args = format!("restore {} --target {}", snapshot_id(&snapshot)?, q(validate::abs_path(target.trim())?));
    job(&app, &state, &id, "Restore", args).await
}

/// Forget specific snapshots and prune their data.
#[tauri::command]
#[specta::specta]
pub async fn restic_forget(app: AppHandle, state: State<'_, AppState>, id: String, snapshots: Vec<String>) -> AppResult<String> {
    if snapshots.is_empty() {
        return Err(AppError::invalid("No snapshots selected"));
    }
    let mut args = String::from("forget --prune");
    for s in &snapshots {
        args.push_str(&format!(" {}", snapshot_id(s)?));
    }
    job(&app, &state, &id, "Forget snapshots", args).await
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeepPolicy {
    pub last: u32,
    pub daily: u32,
    pub weekly: u32,
    pub monthly: u32,
}

impl KeepPolicy {
    pub fn is_empty(&self) -> bool {
        self.last + self.daily + self.weekly + self.monthly == 0
    }

    pub fn args(&self) -> String {
        let mut args = String::new();
        for (flag, n) in
            [("--keep-last", self.last), ("--keep-daily", self.daily), ("--keep-weekly", self.weekly), ("--keep-monthly", self.monthly)]
        {
            if n > 0 {
                args.push_str(&format!(" {flag} {n}"));
            }
        }
        args
    }
}

/// Apply a retention policy (optionally as a dry run that changes nothing).
#[tauri::command]
#[specta::specta]
pub async fn restic_forget_policy(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    policy: KeepPolicy,
    dry_run: bool,
) -> AppResult<String> {
    if policy.is_empty() {
        return Err(AppError::invalid("Set at least one “keep” value, otherwise every snapshot would be removed"));
    }
    let args = format!("forget{}{}", policy.args(), if dry_run { " --dry-run" } else { " --prune" });
    let title = if dry_run { "Retention dry run" } else { "Apply retention policy" };
    job(&app, &state, &id, title, args).await
}

// ---------------------------------------------------------------- snapshot browser

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotNode {
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub size: u64,
    pub mode: u32,
    pub modified: String,
}

/// Parse `restic ls --json` / `restic find --json` node lines.
pub fn parse_nodes(output: &str, only_children_of: Option<&str>) -> Vec<SnapshotNode> {
    let parent = only_children_of.map(|p| p.trim_end_matches('/'));
    let mut nodes = Vec::new();
    let mut push = |v: &serde_json::Value| {
        let (Some(path), Some(kind)) = (v.get("path").and_then(|p| p.as_str()), v.get("type").and_then(|t| t.as_str())) else {
            return;
        };
        if let Some(parent) = parent {
            let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            if dir != parent || path == parent {
                return;
            }
        }
        nodes.push(SnapshotNode {
            name: v.get("name").and_then(|n| n.as_str()).unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path)).to_string(),
            path: path.to_string(),
            dir: kind == "dir",
            size: v.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
            mode: (v.get("mode").and_then(|m| m.as_u64()).unwrap_or(0) & 0o7777) as u32,
            modified: v.get("mtime").and_then(|m| m.as_str()).unwrap_or("").to_string(),
        });
    };
    for line in output.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        match &value {
            // `find` prints an array of { matches: [...] } objects.
            serde_json::Value::Array(groups) => {
                for group in groups {
                    for m in group.get("matches").and_then(|m| m.as_array()).into_iter().flatten() {
                        push(m);
                    }
                }
            }
            v if v.get("struct_type").and_then(|s| s.as_str()) == Some("snapshot") => {}
            v => push(v),
        }
    }
    nodes.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    nodes
}

#[tauri::command]
#[specta::specta]
pub async fn restic_ls(state: State<'_, AppState>, id: String, snapshot: String, path: String) -> AppResult<Vec<SnapshotNode>> {
    let session = state.session()?;
    let repo = repo(&state, &session, &id)?;
    let path = validate::abs_path(&path)?;
    let out = run(&state, &session, &repo, &format!("ls --json {} {}", snapshot_id(&snapshot)?, q(path)), 180).await?;
    Ok(parse_nodes(&out, Some(path)))
}

/// Find files by name pattern within one snapshot.
#[tauri::command]
#[specta::specta]
pub async fn restic_find(state: State<'_, AppState>, id: String, snapshot: String, pattern: String) -> AppResult<Vec<SnapshotNode>> {
    let session = state.session()?;
    let repo = repo(&state, &session, &id)?;
    let pattern = validate::single_line("search text", pattern.trim())?;
    if pattern.is_empty() {
        return Err(AppError::invalid("Enter part of a file name"));
    }
    let out =
        run(&state, &session, &repo, &format!("find --json -i -s {} {}", snapshot_id(&snapshot)?, q(&format!("*{pattern}*"))), 300).await?;
    let mut nodes = parse_nodes(&out, None);
    nodes.truncate(500);
    Ok(nodes)
}

/// Text preview of a file inside a snapshot (size-limited, binary refused).
#[tauri::command]
#[specta::specta]
pub async fn restic_preview(state: State<'_, AppState>, id: String, snapshot: String, path: String, size: u32) -> AppResult<String> {
    if u64::from(size) > PREVIEW_LIMIT {
        return Err(AppError::new(ErrorCode::FileTooLarge, size.to_string()));
    }
    let session = state.session()?;
    let repo = repo(&state, &session, &id)?;
    let out = run(
        &state,
        &session,
        &repo,
        &format!("dump {} {} | head -c {PREVIEW_LIMIT}", snapshot_id(&snapshot)?, q(validate::abs_path(&path)?)),
        180,
    )
    .await?;
    if looks_binary(out.as_bytes()) || out.contains('\u{FFFD}') {
        return Err(AppError::code(ErrorCode::BinaryFile));
    }
    Ok(out)
}

/// Download a file (or a folder, as a tar archive) from a snapshot to this
/// computer: restic writes it to a temporary file on the server, the transfer
/// engine fetches it and removes the temporary file afterwards.
#[tauri::command]
#[specta::specta]
pub async fn restic_download(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    snapshot: String,
    path: String,
    dir: bool,
    local_dir: String,
) -> AppResult<()> {
    let session = state.session()?;
    let repo = repo(&state, &session, &id)?;
    let path = validate::abs_path(&path)?.to_string();
    let snap = snapshot_id(&snapshot)?.to_string();
    let base = path.trim_end_matches('/').rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("snapshot");
    let file_name = if dir { format!("{base}.tar") } else { base.to_string() };
    let staging = format!("{}/.cache/jarvis/restic", session.facts.home.trim_end_matches('/'));
    let remote = format!("{staging}/{}-{file_name}", uuid::Uuid::new_v4().simple());
    let local = std::path::Path::new(&local_dir).join(&file_name).to_string_lossy().into_owned();
    let archive = if dir { "-a tar " } else { "" };
    let args = format!("dump {archive}{snap} {} > {out} || {{ rm -f {out}; exit 1; }}", q(&path), out = q(&remote));
    session.exec(Exec::new(format!("mkdir -p {d} && chmod 700 {d}", d = q(&staging)))).await?.into_stdout()?;
    let exec = restic_exec(&state, &session, &repo, &args)?;
    let meta = JobMeta::visible(format!("Extract from snapshot · {file_name}"), format!("restic dump {archive}{snap} {path}"));
    let app_handle = app.clone();
    state.jobs.spawn(&app, session.clone(), meta, move |ctx| async move {
        ctx.note(format!("Extracting {path} from snapshot {snap}…"));
        let code = ctx.stream(exec).await?;
        if code != 0 {
            return Ok(code);
        }
        ctx.note("Queued for download.");
        let state = app_handle.state::<AppState>();
        transfer::enqueue(
            &app_handle,
            &state,
            ctx.session.clone(),
            vec![TransferRequest {
                kind: TransferKind::Download,
                local,
                remote,
                destination: String::new(),
                conflict: ConflictPolicy::Rename,
                cleanup_remote: true,
            }],
        )
        .await?;
        Ok(0)
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_quote_values_safely() {
        let env = vec![("RESTIC_PASSWORD".to_string(), "it's $ecret `x`".to_string()), ("A".to_string(), String::new())];
        assert_eq!(env_exports(&env), "export RESTIC_PASSWORD='it'\\''s $ecret `x`'\nexport A=''\n");
    }

    #[test]
    fn snapshots_json() {
        let json = r#"[{"time":"2026-10-01T03:00:00.1Z","tree":"t","paths":["/srv/app"],"hostname":"web","username":"root","id":"aaaabbbbccccdddd","short_id":"aaaabbbb","tags":["nightly"]},
                       {"time":"2026-10-02T03:00:00.1Z","paths":["/etc","/home"],"hostname":"web","id":"1111222233334444"}]"#;
        let s = parse_snapshots(json).unwrap();
        assert_eq!(s.len(), 2);
        // Newest first.
        assert_eq!(s[0].short_id, "11112222");
        assert_eq!(s[0].paths, vec!["/etc", "/home"]);
        assert!(s[0].tags.is_empty());
        assert_eq!(s[1].tags, vec!["nightly"]);
        assert!(parse_snapshots("null").unwrap().is_empty());
        assert!(parse_snapshots("").unwrap().is_empty());
        assert!(parse_snapshots("Fatal: wrong password").is_err());
    }

    #[test]
    fn ls_nodes_only_direct_children() {
        let out = r#"{"time":"2026-10-01T03:00:00Z","paths":["/srv"],"id":"aa","struct_type":"snapshot"}
{"name":"app","type":"dir","path":"/srv/app","mode":2147484141,"mtime":"2026-09-30T10:00:00Z","struct_type":"node"}
{"name":"b.txt","type":"file","path":"/srv/b.txt","size":12,"mode":420,"mtime":"2026-09-30T10:00:00Z","struct_type":"node"}
{"name":"deep.txt","type":"file","path":"/srv/app/deep.txt","size":5,"mode":420,"struct_type":"node"}
{"name":"srv","type":"dir","path":"/srv","struct_type":"node"}
"#;
        let nodes = parse_nodes(out, Some("/srv"));
        assert_eq!(nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["app", "b.txt"]);
        assert!(nodes[0].dir);
        assert_eq!(nodes[0].mode, 0o755);
        assert_eq!((nodes[1].size, nodes[1].mode), (12, 0o644));
        assert_eq!(parse_nodes(out, Some("/srv/app/")).len(), 1);
    }

    #[test]
    fn find_matches() {
        let out = r#"[{"matches":[{"path":"/srv/app/config.yml","type":"file","size":80,"mode":420,"mtime":"2026-09-30T10:00:00Z"},{"path":"/srv/config","type":"dir"}],"hits":2,"snapshot":"aa"}]"#;
        let nodes = parse_nodes(out, None);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "config");
        assert_eq!(nodes[1].path, "/srv/app/config.yml");
    }

    #[test]
    fn keep_policy_and_stats() {
        let policy = KeepPolicy { last: 3, daily: 7, weekly: 0, monthly: 12 };
        assert_eq!(policy.args(), " --keep-last 3 --keep-daily 7 --keep-monthly 12");
        assert!(KeepPolicy::default().is_empty());
        let stats = parse_stats(r#"{"total_size":1048576,"total_file_count":42,"snapshots_count":3}"#).unwrap();
        assert_eq!(stats, RepoStats { total_size: 1_048_576, total_file_count: 42, snapshots_count: 3 });
    }

    #[test]
    fn snapshot_ids() {
        assert!(snapshot_id("aaaabbbb").is_ok());
        assert!(snapshot_id("latest").is_ok());
        assert!(snapshot_id("aaaa; rm").is_err());
        assert!(snapshot_id("").is_err());
        assert!(snapshot_id("xyz").is_err());
    }
}
