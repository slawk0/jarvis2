//! Backup templates: files or database dumps, delivered to a folder, an
//! S3/SFTP remote (rclone), a restic repository or this computer, optionally
//! on a schedule installed in root's crontab.
//!
//! One generated shell script serves both "run now" and the schedule. It
//! reads every secret from its environment: sent over stdin for a manual run,
//! sourced from a root-only env file for a scheduled one. Nothing secret is
//! ever placed on a command line.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::cron;
use crate::deps::{self, Tool};
use crate::docker;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::JobMeta;
use crate::restic::{self, KeepPolicy, ResticRepo};
use crate::sftp::transfer::{self, ConflictPolicy, TransferKind, TransferRequest};
use crate::shell::{q, validate};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;
use crate::store::profile_data::DataKey;

const ENV_DIR: &str = "/etc/jarvis-backups";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupTemplate {
    pub id: String,
    pub name: String,
    /// `files`, `mysql` or `postgres`.
    pub kind: String,
    /// Files: the file or directory to archive.
    pub path: String,
    /// Databases: `host` or `container`.
    pub db_source: String,
    pub db_host: String,
    pub db_port: u32,
    pub db_container: String,
    pub db_name: String,
    pub db_user: String,
    /// `download`, `folder`, `s3`, `sftp` or `restic`.
    pub destination: String,
    pub folder: String,
    pub s3_endpoint: String,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_prefix: String,
    pub sftp_host: String,
    pub sftp_port: u32,
    pub sftp_user: String,
    pub sftp_path: String,
    pub restic_repo: String,
    /// Cron expression; empty for manual-only templates.
    pub schedule: String,
    pub paused: bool,
    /// Run the manual backup as root (scheduled runs always are).
    pub sudo: bool,
    /// Delete archives older than this many days (0 keeps everything).
    pub keep_days: u32,
    /// Retention for restic destinations.
    pub keep: KeepPolicy,
}

/// Secret values sent when saving a template. `None` keeps what is stored.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupSecrets {
    pub db_password: Option<String>,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
    pub sftp_password: Option<String>,
}

fn secret(id: &str, name: &str) -> String {
    format!("backup/{id}/{name}")
}

fn script_path(id: &str) -> String {
    format!("/usr/local/bin/jarvis-backup-{id}.sh")
}

fn env_path(id: &str) -> String {
    format!("{ENV_DIR}/{id}.env")
}

fn log_path(id: &str) -> String {
    format!("/var/log/jarvis-backup-{id}.log")
}

fn block_id(id: &str) -> String {
    format!("backup-{id}")
}

/// File-name friendly form of the template name.
pub fn file_slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "backup".to_string()
    } else {
        slug.chars().take(40).collect()
    }
}

fn restic_tag(template: &BackupTemplate) -> String {
    format!("jarvis-{}", &template.id[..template.id.len().min(12)])
}

impl BackupTemplate {
    fn is_db(&self) -> bool {
        self.kind != "files"
    }

    fn in_container(&self) -> bool {
        self.is_db() && self.db_source == "container"
    }

    fn scheduled(&self) -> bool {
        !self.schedule.trim().is_empty()
    }

    fn validate(&self) -> AppResult<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::invalid("Enter a name"));
        }
        validate::single_line("name", &self.name)?;
        validate::one_of("backup type", &self.kind, &["files", "mysql", "postgres"])?;
        validate::one_of("destination", &self.destination, &["download", "folder", "s3", "sftp", "restic"])?;
        if self.is_db() {
            validate::one_of("database source", &self.db_source, &["host", "container"])?;
            if self.in_container() {
                validate::name("container", &self.db_container)?;
            } else {
                validate::host(&self.db_host)?;
                validate::port(self.db_port)?;
            }
            validate::single_line("database name", &self.db_name)?;
            validate::single_line("database user", &self.db_user)?;
            if self.db_name.trim().is_empty() || self.db_user.trim().is_empty() {
                return Err(AppError::invalid("Enter the database name and user"));
            }
        } else {
            let path = validate::abs_path(&self.path)?;
            if path.trim_end_matches('/').is_empty() {
                return Err(AppError::invalid("Backing up the whole filesystem (/) is not supported"));
            }
        }
        match self.destination.as_str() {
            "folder" => {
                validate::abs_path(&self.folder)?;
            }
            "s3" => {
                validate::single_line("endpoint", &self.s3_endpoint)?;
                validate::single_line("region", &self.s3_region)?;
                validate::single_line("bucket", &self.s3_bucket)?;
                validate::single_line("prefix", &self.s3_prefix)?;
                if self.s3_bucket.trim().is_empty() {
                    return Err(AppError::invalid("Enter the bucket name"));
                }
            }
            "sftp" => {
                validate::host(&self.sftp_host)?;
                validate::port(self.sftp_port)?;
                validate::single_line("SFTP user", &self.sftp_user)?;
                validate::single_line("SFTP path", &self.sftp_path)?;
                if self.sftp_user.trim().is_empty() {
                    return Err(AppError::invalid("Enter the SFTP user"));
                }
            }
            "restic" => {
                validate::slug("restic repository", &self.restic_repo)?;
            }
            _ => {}
        }
        if self.scheduled() {
            if self.destination == "download" {
                return Err(AppError::invalid("A backup that downloads to this computer cannot be scheduled"));
            }
            validate::cron_expr(self.schedule.trim())?;
        }
        Ok(())
    }

    /// Server tools the backup needs (clients inside a container are not checked).
    fn tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();
        match (self.kind.as_str(), self.in_container()) {
            ("mysql", false) => tools.push(Tool::MysqlClient),
            ("postgres", false) => tools.push(Tool::PostgresClient),
            (_, true) => tools.push(Tool::Docker),
            _ => {}
        }
        match self.destination.as_str() {
            "s3" | "sftp" => tools.push(Tool::Rclone),
            "restic" => tools.push(Tool::Restic),
            _ => {}
        }
        tools
    }
}

/// How one run of the script is parameterised.
pub struct RunOptions<'a> {
    /// Manual "download" runs: where the finished archive is left for the transfer engine.
    pub download_to: Option<&'a str>,
    /// Manual "download" runs as root: hand the archive to this user.
    pub chown: Option<&'a str>,
    /// The repository of a restic destination (its environment is supplied separately).
    pub restic: Option<&'a ResticRepo>,
    /// The script runs as root but restic must not (so the repository stays
    /// readable for the SSH user): run restic as this user instead.
    pub restic_as: Option<&'a str>,
}

/// The backup as a POSIX shell script. Secrets are read from the environment:
/// `DB_PASSWORD`, `S3_ACCESS_KEY`, `S3_SECRET_KEY`, `SFTP_PASSWORD` and the
/// restic variables.
pub fn build_script(t: &BackupTemplate, options: &RunOptions) -> AppResult<String> {
    let slug = file_slug(&t.name);
    let to_restic = t.destination == "restic";
    let mut s = String::new();
    s.push_str("set -eu\numask 077\n");
    s.push_str("STAMP=$(date +%Y%m%d-%H%M%S)\n");
    s.push_str("WORK=$(mktemp -d \"${TMPDIR:-/tmp}/jarvis-backup.XXXXXX\")\n");
    s.push_str("trap 'rm -rf \"$WORK\"' EXIT\n");
    s.push_str(&format!("NAME={}-$STAMP\n", q(&slug)));

    // 1. Produce the archive (restic reads files directly and dumps from a plain file).
    let extension = if t.is_db() { "sql.gz" } else { "tar.gz" };
    if t.is_db() {
        s.push_str("DUMP=\"$WORK/dump.sql\"\n");
        s.push_str(&format!("echo {}\n", q(&format!("Dumping database {}…", t.db_name.trim()))));
        s.push_str(&dump_command(t)?);
        if !to_restic {
            s.push_str("gzip \"$DUMP\"\nmv \"$DUMP.gz\" \"$WORK/$NAME.sql.gz\"\n");
        }
    } else if !to_restic {
        let path = t.path.trim_end_matches('/');
        let (parent, base) = path.rsplit_once('/').unwrap_or(("", path));
        let parent = if parent.is_empty() { "/" } else { parent };
        s.push_str(&format!("echo {}\n", q(&format!("Archiving {path}…"))));
        s.push_str(&format!("tar -czf \"$WORK/$NAME.tar.gz\" -C {} -- {}\n", q(parent), q(base)));
    }
    if !to_restic {
        s.push_str(&format!("ARTIFACT=\"$WORK/$NAME.{extension}\"\n"));
        s.push_str("echo \"Archive: $NAME.");
        s.push_str(extension);
        s.push_str(" ($(du -h \"$ARTIFACT\" | cut -f1))\"\n");
    }

    // 2. Deliver it, then apply retention.
    let pattern = format!("{slug}-*.{extension}");
    match t.destination.as_str() {
        "download" => {
            let out = options.download_to.ok_or_else(|| AppError::invalid("A download backup can only be run manually"))?;
            s.push_str(&format!("mv \"$ARTIFACT\" {}\n", q(out)));
            if let Some(user) = options.chown {
                s.push_str(&format!("chown {} {}\n", q(user), q(out)));
            }
        }
        "folder" => {
            let dir = q(t.folder.trim_end_matches('/').trim());
            let dir = if dir == "''" { "/".to_string() } else { dir };
            s.push_str(&format!("mkdir -p {dir}\nmv \"$ARTIFACT\" {dir}/\necho \"Saved to \"{dir}\"/$NAME.{extension}\"\n"));
            if t.keep_days > 0 {
                s.push_str(&format!("find {dir} -maxdepth 1 -type f -name {} -mtime +{} -print -delete\n", q(&pattern), t.keep_days));
            }
        }
        "s3" | "sftp" => {
            let remote = if t.destination == "s3" {
                s.push_str("export RCLONE_CONFIG_JARVIS_TYPE=s3\n");
                let provider = if t.s3_endpoint.trim().is_empty() { "AWS" } else { "Other" };
                s.push_str(&format!("export RCLONE_CONFIG_JARVIS_PROVIDER={provider}\n"));
                s.push_str("export RCLONE_CONFIG_JARVIS_ACCESS_KEY_ID=\"$S3_ACCESS_KEY\"\n");
                s.push_str("export RCLONE_CONFIG_JARVIS_SECRET_ACCESS_KEY=\"$S3_SECRET_KEY\"\n");
                if !t.s3_endpoint.trim().is_empty() {
                    s.push_str(&format!("export RCLONE_CONFIG_JARVIS_ENDPOINT={}\n", q(t.s3_endpoint.trim())));
                }
                if !t.s3_region.trim().is_empty() {
                    s.push_str(&format!("export RCLONE_CONFIG_JARVIS_REGION={}\n", q(t.s3_region.trim())));
                }
                let prefix = t.s3_prefix.trim().trim_matches('/');
                format!("jarvis:{}{}", t.s3_bucket.trim(), if prefix.is_empty() { String::new() } else { format!("/{prefix}") })
            } else {
                s.push_str("export RCLONE_CONFIG_JARVIS_TYPE=sftp\n");
                s.push_str(&format!("export RCLONE_CONFIG_JARVIS_HOST={}\n", q(t.sftp_host.trim())));
                s.push_str(&format!("export RCLONE_CONFIG_JARVIS_PORT={}\n", t.sftp_port));
                s.push_str(&format!("export RCLONE_CONFIG_JARVIS_USER={}\n", q(t.sftp_user.trim())));
                // `rclone obscure -` reads the password from stdin; printf is a shell builtin.
                s.push_str("RCLONE_CONFIG_JARVIS_PASS=$(printf %s \"$SFTP_PASSWORD\" | rclone obscure -)\n");
                s.push_str("export RCLONE_CONFIG_JARVIS_PASS\n");
                format!("jarvis:{}", t.sftp_path.trim())
            };
            let remote = q(&remote);
            s.push_str(&format!("echo {}\n", q("Uploading…")));
            s.push_str(&format!("rclone copy \"$ARTIFACT\" {remote}\n"));
            s.push_str("echo \"Uploaded $NAME.");
            s.push_str(extension);
            s.push_str("\"\n");
            if t.keep_days > 0 {
                s.push_str(&format!("rclone delete --min-age {}d --include {} --max-depth 1 {remote}\n", t.keep_days, q(&pattern)));
            }
        }
        "restic" => {
            options.restic.ok_or_else(|| AppError::new(ErrorCode::NotFound, "restic repository"))?;
            let tag = restic_tag(t);
            // Root may always preserve the environment, which carries the repository secrets.
            let restic = match options.restic_as {
                Some(user) => format!("sudo -E -H -u {} restic", q(user)),
                None => "restic".to_string(),
            };
            if t.is_db() {
                s.push_str(&format!("{restic} backup --tag {tag} --stdin --stdin-filename {} < \"$DUMP\"\n", q(&format!("/{slug}.sql"))));
            } else {
                s.push_str(&format!("{restic} backup --tag {tag} {}\n", q(t.path.trim())));
            }
            if !t.keep.is_empty() {
                s.push_str(&format!("{restic} forget --tag {tag}{} --prune\n", t.keep.args()));
            }
        }
        _ => unreachable!("validated"),
    }
    s.push_str("echo 'Backup finished.'\n");
    Ok(s)
}

/// Shell lines that write the database dump to `$DUMP`.
fn dump_command(t: &BackupTemplate) -> AppResult<String> {
    let (db, user) = (q(t.db_name.trim()), q(t.db_user.trim()));
    Ok(match (t.kind.as_str(), t.in_container()) {
        ("mysql", false) => format!(
            // Oracle's mysqldump 8 queries COLUMN_STATISTICS, which MariaDB and MySQL 5.7 lack.
            "DUMPER=$(command -v mysqldump || command -v mariadb-dump)\n\
             STATS=\n\
             if \"$DUMPER\" --help 2>/dev/null | grep -q column-statistics; then STATS=--column-statistics=0; fi\n\
             MYSQL_PWD=\"$DB_PASSWORD\" \"$DUMPER\" $STATS -h {} -P {} -u {user} --single-transaction --routines --triggers {db} > \"$DUMP\"\n",
            q(t.db_host.trim()),
            t.db_port
        ),
        ("mysql", true) => format!(
            "MYSQL_PWD=\"$DB_PASSWORD\" docker exec -e MYSQL_PWD {} sh -c {} sh -u {user} --single-transaction --routines --triggers {db} > \"$DUMP\"\n",
            q(&t.db_container),
            q("if command -v mariadb-dump >/dev/null 2>&1; then exec mariadb-dump \"$@\"; else exec mysqldump \"$@\"; fi"),
        ),
        ("postgres", false) => format!(
            "PGPASSWORD=\"$DB_PASSWORD\" pg_dump -h {} -p {} -U {user} {db} > \"$DUMP\"\n",
            q(t.db_host.trim()),
            t.db_port
        ),
        ("postgres", true) => format!(
            "PGPASSWORD=\"$DB_PASSWORD\" docker exec -e PGPASSWORD {} pg_dump -U {user} {db} > \"$DUMP\"\n",
            q(&t.db_container)
        ),
        _ => return Err(AppError::invalid("Unknown backup type")),
    })
}

/// The script installed for a schedule: loads the env file and logs a frame around the run.
pub fn scheduled_script(t: &BackupTemplate, body: &str) -> String {
    format!(
        "#!/bin/sh\n# Managed by Jarvis: backup \"{}\". Changes are overwritten.\n\
         PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n\
         export LC_ALL=C\n\
         . {}\n\
         echo \"=== $(date '+%Y-%m-%d %H:%M:%S') backup started ===\"\n\
         (\n{body})\ncode=$?\n\
         echo \"=== $(date '+%Y-%m-%d %H:%M:%S') backup finished (exit $code) ===\"\n\
         exit $code\n",
        t.name.replace(['"', '\n', '\r'], " "),
        q(&env_path(&t.id)),
    )
}

pub fn cron_line(t: &BackupTemplate) -> String {
    format!("{} {} >> {} 2>&1", t.schedule.trim(), script_path(&t.id), log_path(&t.id))
}

fn templates(state: &AppState, session: &Session) -> AppResult<Vec<BackupTemplate>> {
    state.data.get_as(&session.profile.id, DataKey::BackupTemplates)
}

fn template(state: &AppState, session: &Session, id: &str) -> AppResult<BackupTemplate> {
    templates(state, session)?.into_iter().find(|t| t.id == id).ok_or_else(|| AppError::new(ErrorCode::NotFound, "backup template"))
}

fn restic_repo(state: &AppState, session: &Session, t: &BackupTemplate) -> AppResult<Option<ResticRepo>> {
    if t.destination == "restic" {
        restic::repo(state, session, &t.restic_repo).map(Some)
    } else {
        Ok(None)
    }
}

/// Restic runs as root only when the repository or the template asks for it;
/// otherwise a root-run script hands restic back to the SSH user.
fn restic_as_root(t: &BackupTemplate, repo: Option<&ResticRepo>) -> bool {
    t.sudo || repo.is_some_and(|r| r.sudo)
}

/// Every secret the script expects, from the keyring.
fn environment(state: &AppState, session: &Session, t: &BackupTemplate, repo: Option<&ResticRepo>) -> AppResult<Vec<(String, String)>> {
    let owner = &session.profile.id;
    let get = |name: &str| state.secrets.get_or_empty(owner, &secret(&t.id, name));
    let mut env = Vec::new();
    if t.is_db() {
        env.push(("DB_PASSWORD".to_string(), get("db-password")?));
    }
    match t.destination.as_str() {
        "s3" => {
            env.push(("S3_ACCESS_KEY".to_string(), get("s3-access-key")?));
            env.push(("S3_SECRET_KEY".to_string(), get("s3-secret-key")?));
        }
        "sftp" => env.push(("SFTP_PASSWORD".to_string(), get("sftp-password")?)),
        _ => {}
    }
    if let Some(repo) = repo {
        env.extend(restic::environment(state, session, repo)?);
    }
    Ok(env)
}

/// Install, refresh or remove the schedule on the server to match the template.
async fn sync_schedule(state: &AppState, session: &Session, t: &BackupTemplate) -> AppResult<()> {
    let (script, env, id) = (q(&script_path(&t.id)), q(&env_path(&t.id)), block_id(&t.id));
    if !t.scheduled() {
        // Nothing installed for a template that never had a schedule: skip the sudo round trip.
        let installed = session.exec(Exec::new(format!("[ -e {script} ] || [ -e {env} ]"))).await?.success();
        if !installed {
            return Ok(());
        }
        session.exec(Exec::new(format!("rm -f {script} {env}")).sudo()).await?.into_stdout()?;
        let crontab = cron::read(session, true).await?;
        let next = cron::remove_block(&crontab, &id);
        if next != crontab {
            cron::write(session, &next, true).await?;
        }
        return Ok(());
    }
    let repo = restic_repo(state, session, t)?;
    let restic_as = (!session.is_root() && !restic_as_root(t, repo.as_ref())).then_some(session.facts.user.as_str());
    let body = build_script(t, &RunOptions { download_to: None, chown: None, restic: repo.as_ref(), restic_as })?;
    let file = scheduled_script(t, &body);
    let exports = restic::env_exports(&environment(state, session, t, repo.as_ref())?);
    // The env file is created empty with mode 600 before the secrets are written into it.
    let install = format!(
        "mkdir -p {dir} && chmod 700 {dir} && (umask 077; : > {env}) && chmod 600 {env} && cat > {env} \
         && printf '%s' {content} > {script} && chmod 755 {script}",
        dir = q(ENV_DIR),
        content = q(&file),
    );
    session.exec(Exec::new(install).sudo().stdin(exports)).await?.into_stdout()?;
    let crontab = cron::read(session, true).await?;
    let next = if t.paused { cron::remove_block(&crontab, &id) } else { cron::upsert_block(&crontab, &id, &[cron_line(t)]) };
    if next != crontab {
        cron::write(session, &next, true).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------- commands

#[tauri::command]
#[specta::specta]
pub fn backup_templates(state: State<'_, AppState>) -> AppResult<Vec<BackupTemplate>> {
    let session = state.session()?;
    templates(&state, &session)
}

#[tauri::command]
#[specta::specta]
pub async fn backup_template_save(
    state: State<'_, AppState>,
    template: BackupTemplate,
    secrets: BackupSecrets,
) -> AppResult<BackupTemplate> {
    let session = state.session()?;
    let mut t = template;
    t.name = t.name.trim().to_string();
    if t.id.is_empty() {
        t.id = uuid::Uuid::new_v4().simple().to_string();
    } else {
        validate::slug("template id", &t.id)?;
    }
    t.validate()?;
    if t.destination == "restic" {
        restic::repo(&state, &session, &t.restic_repo)?;
    }
    let previous = templates(&state, &session)?.into_iter().find(|p| p.id == t.id);
    let touches_schedule = t.scheduled() || previous.as_ref().is_some_and(|p| p.scheduled());
    if touches_schedule && !session.is_root() {
        // Ask for the password before anything is stored, so a retry starts clean.
        session.sudo_plan().await?;
    }
    let owner = session.profile.id.clone();
    for (name, value) in [
        ("db-password", &secrets.db_password),
        ("s3-access-key", &secrets.s3_access_key),
        ("s3-secret-key", &secrets.s3_secret_key),
        ("sftp-password", &secrets.sftp_password),
    ] {
        if let Some(value) = value {
            state.secrets.set(&owner, &secret(&t.id, name), value)?;
        }
    }
    let stored = t.clone();
    state.data.update(&owner, DataKey::BackupTemplates, |all: &mut Vec<BackupTemplate>| {
        match all.iter_mut().find(|x| x.id == stored.id) {
            Some(existing) => *existing = stored,
            None => all.push(stored),
        }
    })?;
    if touches_schedule {
        sync_schedule(&state, &session, &t).await?;
    }
    Ok(t)
}

#[tauri::command]
#[specta::specta]
pub async fn backup_template_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let session = state.session()?;
    let mut t = template(&state, &session, &id)?;
    // Removing the schedule first: if it fails, the template is still there to retry.
    t.schedule.clear();
    sync_schedule(&state, &session, &t).await?;
    let owner = session.profile.id.clone();
    state.secrets.delete_prefixed(&owner, &format!("backup/{id}/"))?;
    state.data.update(&owner, DataKey::BackupTemplates, |all: &mut Vec<BackupTemplate>| all.retain(|x| x.id != id))
}

/// Pause or resume a schedule (the installed script stays in place).
#[tauri::command]
#[specta::specta]
pub async fn backup_set_paused(state: State<'_, AppState>, id: String, paused: bool) -> AppResult<()> {
    let session = state.session()?;
    let mut t = template(&state, &session, &id)?;
    if !t.scheduled() {
        return Err(AppError::invalid("This backup has no schedule"));
    }
    t.paused = paused;
    sync_schedule(&state, &session, &t).await?;
    state.data.update(&session.profile.id, DataKey::BackupTemplates, |all: &mut Vec<BackupTemplate>| {
        if let Some(existing) = all.iter_mut().find(|x| x.id == id) {
            existing.paused = paused;
        }
    })
}

/// Run a backup now as a streamed job. `local_dir` is required for download destinations.
#[tauri::command]
#[specta::specta]
pub async fn backup_run(app: AppHandle, state: State<'_, AppState>, id: String, local_dir: Option<String>) -> AppResult<String> {
    let session = state.session()?;
    let t = template(&state, &session, &id)?;
    t.validate()?;
    for tool in t.tools() {
        deps::require(&session, tool).await?;
    }
    let repo = restic_repo(&state, &session, &t)?;
    let sudo = !session.is_root()
        && (t.sudo || repo.as_ref().is_some_and(|r| r.sudo) || (t.in_container() && docker::needs_sudo(&session).await?));

    let download = if t.destination == "download" {
        let local_dir = local_dir.ok_or_else(|| AppError::invalid("Choose a folder to download to"))?;
        let extension = if t.is_db() { "sql.gz" } else { "tar.gz" };
        let file = format!("{}-{}.{extension}", file_slug(&t.name), chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let staging = format!("{}/.cache/jarvis/backups", session.facts.home.trim_end_matches('/'));
        session.exec(Exec::new(format!("mkdir -p {d} && chmod 700 {d}", d = q(&staging)))).await?.into_stdout()?;
        let local = std::path::Path::new(&local_dir).join(&file).to_string_lossy().into_owned();
        Some((format!("{staging}/{file}"), local))
    } else {
        None
    };

    let script = build_script(
        &t,
        &RunOptions {
            download_to: download.as_ref().map(|(remote, _)| remote.as_str()),
            chown: sudo.then_some(session.facts.user.as_str()),
            restic: repo.as_ref(),
            restic_as: (sudo && !restic_as_root(&t, repo.as_ref())).then_some(session.facts.user.as_str()),
        },
    )?;
    let exports = restic::env_exports(&environment(&state, &session, &t, repo.as_ref())?);
    let exec = Exec::new(format!("eval \"$(cat)\"\n{{\n{script}}} 2>&1")).stdin(exports).sudo_if(sudo);
    if sudo {
        session.sudo_plan().await?;
    }
    let meta = JobMeta::visible(format!("Backup · {}", t.name), format!("{} → {}", t.kind, t.destination));
    let app_handle = app.clone();
    Ok(state.jobs.spawn(&app, session, meta, move |ctx| async move {
        let code = ctx.stream(exec).await?;
        let Some((remote, local)) = download else {
            return Ok(code);
        };
        if code != 0 {
            let _ = ctx.session.exec(Exec::new(format!("rm -f {}", q(&remote))).sudo_if(sudo)).await;
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
    }))
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInfo {
    pub id: String,
    /// The script and env file exist on the server.
    pub installed: bool,
    /// The line in root's crontab; empty when paused or missing.
    pub cron_line: String,
    pub log: String,
}

/// State of an installed schedule with the tail of its log.
#[tauri::command]
#[specta::specta]
pub async fn backup_schedule_info(state: State<'_, AppState>, id: String, lines: u32) -> AppResult<ScheduleInfo> {
    let session = state.session()?;
    validate::slug("template id", &id)?;
    let script = format!(
        "[ -e {s} ] && echo yes || echo no; echo '[[cron]]'; crontab -l 2>/dev/null || true; echo '[[log]]'; tail -n {} {l} 2>/dev/null || true",
        lines.clamp(10, 5000),
        s = q(&script_path(&id)),
        l = q(&log_path(&id)),
    );
    let out = session.exec(Exec::new(script).sudo()).await?.into_stdout()?;
    let (head, rest) = out.split_once("[[cron]]\n").unwrap_or((&out, ""));
    let (crontab, log) = rest.split_once("[[log]]\n").unwrap_or((rest, ""));
    Ok(ScheduleInfo {
        installed: head.trim() == "yes",
        cron_line: cron::block_lines(crontab, &block_id(&id)).and_then(|lines| lines.into_iter().next()).unwrap_or_default(),
        log: log.to_string(),
        id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> BackupTemplate {
        BackupTemplate {
            id: "abc123".into(),
            name: "Web App (prod)".into(),
            kind: "files".into(),
            path: "/srv/app/".into(),
            db_source: "host".into(),
            db_host: "127.0.0.1".into(),
            db_port: 3306,
            db_container: String::new(),
            db_name: "shop".into(),
            db_user: "shop".into(),
            destination: "folder".into(),
            folder: "/var/backups/app".into(),
            s3_endpoint: String::new(),
            s3_region: String::new(),
            s3_bucket: String::new(),
            s3_prefix: String::new(),
            sftp_host: String::new(),
            sftp_port: 22,
            sftp_user: String::new(),
            sftp_path: String::new(),
            restic_repo: String::new(),
            schedule: String::new(),
            paused: false,
            sudo: false,
            keep_days: 0,
            keep: KeepPolicy::default(),
        }
    }

    fn none() -> RunOptions<'static> {
        RunOptions { download_to: None, chown: None, restic: None, restic_as: None }
    }

    #[test]
    fn slugs() {
        assert_eq!(file_slug("Web App (prod)"), "web-app-prod");
        assert_eq!(file_slug("  ąę  "), "backup");
        assert_eq!(file_slug("a'; rm -rf /"), "a-rm-rf");
    }

    #[test]
    fn files_to_folder_with_retention() {
        let mut t = base();
        t.keep_days = 14;
        t.validate().unwrap();
        let s = build_script(&t, &none()).unwrap();
        assert!(s.starts_with("set -eu\numask 077\n"));
        assert!(s.contains("NAME=web-app-prod-$STAMP\n"));
        assert!(s.contains("tar -czf \"$WORK/$NAME.tar.gz\" -C /srv -- app\n"));
        assert!(s.contains("mv \"$ARTIFACT\" /var/backups/app/\n"));
        assert!(s.contains("find /var/backups/app -maxdepth 1 -type f -name 'web-app-prod-*.tar.gz' -mtime +14 -print -delete\n"));
        t.keep_days = 0;
        assert!(!build_script(&t, &none()).unwrap().contains("find "));
    }

    #[test]
    fn database_dumps_never_put_the_password_on_a_command_line() {
        let mut t = base();
        t.kind = "mysql".into();
        let s = build_script(&t, &none()).unwrap();
        assert!(s.contains("MYSQL_PWD=\"$DB_PASSWORD\" \"$DUMPER\" $STATS -h 127.0.0.1 -P 3306 -u shop --single-transaction --routines --triggers shop > \"$DUMP\"\n"));
        assert!(s.contains("mv \"$DUMP.gz\" \"$WORK/$NAME.sql.gz\"\n"));

        t.db_source = "container".into();
        t.db_container = "db".into();
        let s = build_script(&t, &none()).unwrap();
        assert!(s.contains("MYSQL_PWD=\"$DB_PASSWORD\" docker exec -e MYSQL_PWD db sh -c "));
        assert!(s.contains("mariadb-dump"));

        t.kind = "postgres".into();
        let s = build_script(&t, &none()).unwrap();
        assert!(s.contains("PGPASSWORD=\"$DB_PASSWORD\" docker exec -e PGPASSWORD db pg_dump -U shop shop > \"$DUMP\"\n"));
        t.db_source = "host".into();
        t.db_port = 5432;
        let s = build_script(&t, &none()).unwrap();
        assert!(s.contains("PGPASSWORD=\"$DB_PASSWORD\" pg_dump -h 127.0.0.1 -p 5432 -U shop shop > \"$DUMP\"\n"));
    }

    #[test]
    fn s3_and_sftp_use_rclone_from_the_environment() {
        let mut t = base();
        t.destination = "s3".into();
        t.s3_endpoint = "https://s3.example.com".into();
        t.s3_bucket = "backups".into();
        t.s3_prefix = "/web/".into();
        t.keep_days = 30;
        t.validate().unwrap();
        let s = build_script(&t, &none()).unwrap();
        assert!(s.contains("export RCLONE_CONFIG_JARVIS_PROVIDER=Other\n"));
        assert!(s.contains("export RCLONE_CONFIG_JARVIS_SECRET_ACCESS_KEY=\"$S3_SECRET_KEY\"\n"));
        assert!(s.contains("export RCLONE_CONFIG_JARVIS_ENDPOINT=https://s3.example.com\n"));
        assert!(s.contains("rclone copy \"$ARTIFACT\" jarvis:backups/web\n"));
        assert!(s.contains("rclone delete --min-age 30d --include 'web-app-prod-*.tar.gz' --max-depth 1 jarvis:backups/web\n"));

        t.destination = "sftp".into();
        t.sftp_host = "nas.example.com".into();
        t.sftp_user = "backup".into();
        t.sftp_path = "/volume1/backups".into();
        t.validate().unwrap();
        let s = build_script(&t, &none()).unwrap();
        assert!(s.contains("RCLONE_CONFIG_JARVIS_PASS=$(printf %s \"$SFTP_PASSWORD\" | rclone obscure -)\n"));
        assert!(s.contains("rclone copy \"$ARTIFACT\" jarvis:/volume1/backups\n"));
    }

    #[test]
    fn restic_destination() {
        let repo = ResticRepo {
            id: "r1".into(),
            name: "Repo".into(),
            kind: "local".into(),
            repository: "/srv/restic".into(),
            fields: Default::default(),
            env_names: vec![],
            sudo: false,
        };
        let mut t = base();
        t.destination = "restic".into();
        t.restic_repo = "r1".into();
        t.keep = KeepPolicy { last: 0, daily: 7, weekly: 4, monthly: 0 };
        let options = RunOptions { download_to: None, chown: None, restic: Some(&repo), restic_as: None };
        let s = build_script(&t, &options).unwrap();
        assert!(!s.contains("tar "));
        assert!(s.contains("restic backup --tag jarvis-abc123 /srv/app/\n"));
        assert!(s.contains("restic forget --tag jarvis-abc123 --keep-daily 7 --keep-weekly 4 --prune\n"));
        t.kind = "postgres".into();
        let s = build_script(&t, &options).unwrap();
        assert!(s.contains("restic backup --tag jarvis-abc123 --stdin --stdin-filename /web-app-prod.sql < \"$DUMP\"\n"));
        assert!(!s.contains("gzip"));
        let as_user = RunOptions { restic_as: Some("deploy"), ..options };
        let s = build_script(&t, &as_user).unwrap();
        assert!(s.contains("sudo -E -H -u deploy restic backup --tag jarvis-abc123 --stdin"));
        assert!(s.contains("sudo -E -H -u deploy restic forget --tag jarvis-abc123 --keep-daily 7 --keep-weekly 4 --prune"));
        assert!(build_script(&t, &none()).is_err());
    }

    #[test]
    fn download_needs_a_manual_run() {
        let mut t = base();
        t.destination = "download".into();
        assert!(build_script(&t, &none()).is_err());
        let s = build_script(
            &t,
            &RunOptions { restic_as: None, download_to: Some("/home/a/.cache/jarvis/backups/x.tar.gz"), chown: Some("a"), restic: None },
        )
        .unwrap();
        assert!(s.contains("mv \"$ARTIFACT\" /home/a/.cache/jarvis/backups/x.tar.gz\nchown a /home/a/.cache/jarvis/backups/x.tar.gz\n"));
        t.schedule = "0 3 * * *".into();
        assert!(t.validate().is_err());
    }

    #[test]
    fn validation() {
        let mut t = base();
        t.path = "/".into();
        assert!(t.validate().is_err());
        t.path = "relative".into();
        assert!(t.validate().is_err());
        let mut t = base();
        t.schedule = "not cron".into();
        assert!(t.validate().is_err());
        t.schedule = "30 2 * * 0".into();
        t.validate().unwrap();
        assert_eq!(cron_line(&t), "30 2 * * 0 /usr/local/bin/jarvis-backup-abc123.sh >> /var/log/jarvis-backup-abc123.log 2>&1");
        let mut t = base();
        t.kind = "mysql".into();
        t.db_name = " ".into();
        assert!(t.validate().is_err());
    }

    #[test]
    fn scheduled_wrapper() {
        let t = base();
        let file = scheduled_script(&t, "set -eu\necho hi\n");
        assert!(file.starts_with("#!/bin/sh\n# Managed by Jarvis: backup \"Web App (prod)\"."));
        assert!(file.contains(". /etc/jarvis-backups/abc123.env\n"));
        assert!(file.contains("(\nset -eu\necho hi\n)\ncode=$?\n"));
        assert!(file.ends_with("exit $code\n"));
    }
}
