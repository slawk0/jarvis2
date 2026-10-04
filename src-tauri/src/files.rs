//! Remote file management: listing, editing and file operations.
//!
//! Everything here runs as shell commands (not SFTP) so the same code path
//! works as the login user and, transparently, as root through the sudo
//! service when the user lacks permission. Bulk byte transfer lives in
//! `sftp::transfer`.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::shell::{q, validate};
use crate::ssh::session::{Exec, Output, Session};
use crate::state::AppState;
use crate::text::looks_binary;

pub const MAX_EDIT_BYTES: u64 = 5 * 1024 * 1024;
const SEARCH_LIMIT: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    /// Full path; only differs from `dir + name` in search results.
    pub path: String,
    pub kind: FileKind,
    /// For symlinks: whether the link points at a directory.
    pub is_dir_like: bool,
    pub size: u64,
    /// Permission bits (lower 12 bits of the mode).
    pub mode: u32,
    pub owner: String,
    pub group: String,
    /// Modification time, Unix seconds.
    pub modified: i64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    pub entries: Vec<FileEntry>,
    /// The listing needed root.
    pub elevated: bool,
    pub truncated: bool,
}

/// `stat` format: raw mode (hex), size, owner, group, mtime, then the name.
/// Fields are tab separated; the name comes last so it may contain spaces.
const STAT_FORMAT: &str = "%f\t%s\t%U\t%G\t%Y\t%n";

pub fn parse_stat_line(line: &str, dir: &str) -> Option<FileEntry> {
    let mut fields = line.splitn(6, '\t');
    let raw_mode = u32::from_str_radix(fields.next()?, 16).ok()?;
    let size = fields.next()?.parse().ok()?;
    let owner = fields.next()?.to_string();
    let group = fields.next()?.to_string();
    let modified = fields.next()?.parse().ok()?;
    let name_field = fields.next()?;
    // Search results carry full paths; directory listings carry bare names.
    let (name, path) = if name_field.starts_with('/') {
        let name = name_field.rsplit('/').next().unwrap_or(name_field);
        (name.to_string(), name_field.to_string())
    } else {
        let name = name_field.strip_prefix("./").unwrap_or(name_field);
        (name.to_string(), join(dir, name))
    };
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let kind = match raw_mode & 0o170000 {
        0o040000 => FileKind::Dir,
        0o120000 => FileKind::Symlink,
        0o100000 => FileKind::File,
        _ => FileKind::Other,
    };
    Some(FileEntry { name, path, kind, is_dir_like: kind == FileKind::Dir, size, mode: raw_mode & 0o7777, owner, group, modified })
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

pub fn parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(i) => trimmed[..i].to_string(),
    }
}

fn list_script(dir: &str) -> String {
    // Exit 3: cannot enter; exit 4: cannot read. Both mean "try as root".
    // The second stat marks symlinks that resolve to directories.
    format!(
        "cd {d} 2>/dev/null || {{ [ -e {d} ] && exit 3; exit 5; }}; ls -A >/dev/null 2>&1 || exit 4; pwd; \
         stat -c '{STAT_FORMAT}' -- .[!.]* ..?* * 2>/dev/null; echo '#links'; \
         for f in .[!.]* ..?* *; do [ -L \"$f\" ] && [ -d \"$f\" ] && printf '%s\\n' \"$f\"; done; true",
        d = q(dir)
    )
}

fn parse_listing(stdout: &str, elevated: bool) -> Listing {
    let mut lines = stdout.lines();
    let path = lines.next().unwrap_or("/").to_string();
    let mut entries = Vec::new();
    let mut dir_links: Vec<&str> = Vec::new();
    let mut in_links = false;
    for line in lines {
        if line == "#links" {
            in_links = true;
        } else if in_links {
            dir_links.push(line);
        } else if let Some(entry) = parse_stat_line(line, &path) {
            entries.push(entry);
        }
    }
    for entry in &mut entries {
        if entry.kind == FileKind::Symlink && dir_links.contains(&entry.name.as_str()) {
            entry.is_dir_like = true;
        }
    }
    Listing { path, entries, elevated, truncated: false }
}

fn listing_error(dir: &str, out: &Output) -> AppError {
    match out.code {
        5 => AppError::new(ErrorCode::NotFound, dir),
        3 | 4 => AppError::new(ErrorCode::PermissionDenied, dir),
        _ => AppError::new(ErrorCode::CommandFailed, out.failure_text()),
    }
}

/// List a directory, retrying as root when the user may not read it.
pub async fn list(session: &Session, dir: &str) -> AppResult<Listing> {
    validate::abs_path(dir)?;
    let exec = Exec::new(list_script(dir));
    let out = session.exec(exec.clone()).await?;
    if out.success() {
        return Ok(parse_listing(&out.stdout, false));
    }
    if matches!(out.code, 3 | 4) && !session.is_root() {
        let out = session.exec(exec.sudo()).await?;
        if out.success() {
            return Ok(parse_listing(&out.stdout, true));
        }
        return Err(listing_error(dir, &out));
    }
    Err(listing_error(dir, &out))
}

/// Run a file operation, elevating on permission errors. Returns whether root was needed.
async fn run_op(session: &Session, script: String, timeout_secs: u64) -> AppResult<bool> {
    let exec = Exec::new(script).secs(timeout_secs);
    if session.is_root() {
        session.exec(exec).await?.into_stdout()?;
        return Ok(false);
    }
    let out = session.exec(exec.clone()).await?;
    if out.success() {
        return Ok(false);
    }
    if out.permission_denied() {
        session.exec(exec.sudo()).await?.into_stdout()?;
        return Ok(true);
    }
    Err(out.into_stdout().unwrap_err())
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub path: String,
    pub content: String,
    /// The file could only be read as root; saving will need root too.
    pub elevated: bool,
    pub size: u64,
}

/// Read a text file for the editor (size-limited, binary files refused).
pub async fn read_text(session: &Session, path: &str, max_bytes: u64) -> AppResult<FileContent> {
    validate::abs_path(path)?;
    let p = q(path);
    // Exit 6: not a regular file. Size is checked before any content is sent.
    let script = format!(
        "[ -e {p} ] || exit 5; [ -f {p} ] || exit 6; s=$(stat -c %s -- {p}) || exit 4; \
         [ \"$s\" -le {max_bytes} ] || {{ echo \"$s\"; exit 7; }}; [ -r {p} ] || exit 4; echo \"$s\"; cat -- {p}"
    );
    let exec = Exec::new(script).secs(60);
    let mut elevated = false;
    let mut out = session.exec(exec.clone()).await?;
    if out.code == 4 && !session.is_root() {
        out = session.exec(exec.sudo()).await?;
        elevated = true;
    }
    match out.code {
        0 => {}
        5 => return Err(AppError::new(ErrorCode::NotFound, path)),
        6 => return Err(AppError::invalid("Not a regular file")),
        7 => return Err(AppError::new(ErrorCode::FileTooLarge, out.stdout.trim())),
        4 => return Err(AppError::new(ErrorCode::PermissionDenied, path)),
        _ => return Err(AppError::new(ErrorCode::CommandFailed, out.failure_text())),
    }
    let (size_line, content) = out.stdout.split_once('\n').unwrap_or((&out.stdout, ""));
    if looks_binary(content.as_bytes()) || content.contains('\u{FFFD}') {
        return Err(AppError::code(ErrorCode::BinaryFile));
    }
    Ok(FileContent {
        path: path.to_string(),
        size: size_line.trim().parse().unwrap_or(content.len() as u64),
        content: content.to_string(),
        elevated,
    })
}

/// Write a text file in place (keeps owner and permissions). Returns whether root was needed.
pub async fn write_text(session: &Session, path: &str, content: &str) -> AppResult<bool> {
    validate::abs_path(path)?;
    let exec = Exec::new(format!("cat > {}", q(path))).stdin(content.as_bytes().to_vec()).secs(60);
    if session.is_root() {
        session.exec(exec).await?.into_stdout()?;
        return Ok(false);
    }
    let out = session.exec(exec.clone()).await?;
    if out.success() {
        return Ok(false);
    }
    if out.permission_denied() {
        session.exec(exec.sudo()).await?.into_stdout()?;
        return Ok(true);
    }
    Err(out.into_stdout().unwrap_err())
}

fn paths_arg(paths: &[String]) -> AppResult<String> {
    if paths.is_empty() {
        return Err(AppError::invalid("No paths given"));
    }
    let mut quoted = Vec::with_capacity(paths.len());
    for path in paths {
        validate::abs_path(path)?;
        if path.trim_end_matches('/').is_empty() {
            return Err(AppError::invalid("Refusing to operate on /"));
        }
        quoted.push(q(path));
    }
    Ok(quoted.join(" "))
}

// ---------------------------------------------------------------- commands

#[tauri::command]
#[specta::specta]
pub async fn files_list(state: State<'_, AppState>, path: String) -> AppResult<Listing> {
    let session = state.session()?;
    list(&session, &path).await
}

/// The connected user's home directory (where the file browser starts).
#[tauri::command]
#[specta::specta]
pub fn files_home(state: State<'_, AppState>) -> AppResult<String> {
    Ok(state.session()?.facts.home.clone())
}

/// Path completions for the remote path input: entries of the typed
/// directory whose names start with the typed prefix.
#[tauri::command]
#[specta::specta]
pub async fn files_complete(state: State<'_, AppState>, input: String, dirs_only: bool) -> AppResult<Vec<String>> {
    let session = state.session()?;
    if !input.starts_with('/') {
        return Ok(Vec::new());
    }
    let (dir, prefix) = match input.rfind('/') {
        Some(0) => ("/", &input[1..]),
        Some(i) => (&input[..i], &input[i + 1..]),
        None => return Ok(Vec::new()),
    };
    let Ok(listing) = list(&session, dir).await else {
        return Ok(Vec::new());
    };
    let needle = prefix.to_lowercase();
    let mut matches: Vec<&FileEntry> = listing
        .entries
        .iter()
        .filter(|e| !dirs_only || e.is_dir_like)
        .filter(|e| e.name.to_lowercase().starts_with(&needle))
        .filter(|e| prefix.starts_with('.') || !e.name.starts_with('.'))
        .collect();
    matches.sort_by(|a, b| b.is_dir_like.cmp(&a.is_dir_like).then(a.name.cmp(&b.name)));
    Ok(matches.into_iter().take(50).map(|e| if e.is_dir_like { format!("{}/", e.path) } else { e.path.clone() }).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn files_read(state: State<'_, AppState>, path: String) -> AppResult<FileContent> {
    let session = state.session()?;
    read_text(&session, &path, MAX_EDIT_BYTES).await
}

#[tauri::command]
#[specta::specta]
pub async fn files_write(state: State<'_, AppState>, path: String, content: String) -> AppResult<bool> {
    let session = state.session()?;
    write_text(&session, &path, &content).await
}

#[tauri::command]
#[specta::specta]
pub async fn files_create(state: State<'_, AppState>, dir: String, name: String, directory: bool) -> AppResult<bool> {
    let session = state.session()?;
    validate::abs_path(&dir)?;
    validate::file_name(&name)?;
    let target = q(&join(&dir, &name));
    let script = if directory {
        format!("[ ! -e {target} ] || {{ echo 'Already exists' >&2; exit 1; }}; mkdir -- {target}")
    } else {
        format!("[ ! -e {target} ] || {{ echo 'Already exists' >&2; exit 1; }}; : > {target}")
    };
    run_op(&session, script, 30).await
}

#[tauri::command]
#[specta::specta]
pub async fn files_rename(state: State<'_, AppState>, path: String, new_name: String) -> AppResult<bool> {
    let session = state.session()?;
    validate::abs_path(&path)?;
    validate::file_name(&new_name)?;
    let target = q(&join(&parent(&path), &new_name));
    let script =
        format!("[ ! -e {target} ] || {{ echo 'A file with that name already exists' >&2; exit 1; }}; mv -- {} {target}", q(&path));
    run_op(&session, script, 30).await
}

/// Move or copy items into a directory.
#[tauri::command]
#[specta::specta]
pub async fn files_transfer(state: State<'_, AppState>, paths: Vec<String>, destination: String, copy: bool) -> AppResult<bool> {
    let session = state.session()?;
    validate::abs_path(&destination)?;
    let sources = paths_arg(&paths)?;
    let tool = if copy { "cp -a" } else { "mv" };
    run_op(&session, format!("{tool} -- {sources} {}/", q(destination.trim_end_matches('/'))), 600).await
}

/// Copy an item next to itself under a new name.
#[tauri::command]
#[specta::specta]
pub async fn files_duplicate(state: State<'_, AppState>, path: String, new_name: String) -> AppResult<bool> {
    let session = state.session()?;
    validate::abs_path(&path)?;
    validate::file_name(&new_name)?;
    let target = q(&join(&parent(&path), &new_name));
    let script =
        format!("[ ! -e {target} ] || {{ echo 'A file with that name already exists' >&2; exit 1; }}; cp -a -- {} {target}", q(&path));
    run_op(&session, script, 600).await
}

#[tauri::command]
#[specta::specta]
pub async fn files_delete(state: State<'_, AppState>, paths: Vec<String>) -> AppResult<bool> {
    let session = state.session()?;
    let targets = paths_arg(&paths)?;
    run_op(&session, format!("rm -rf -- {targets}"), 600).await
}

#[tauri::command]
#[specta::specta]
pub async fn files_chmod(state: State<'_, AppState>, paths: Vec<String>, mode: u32, recursive: bool) -> AppResult<bool> {
    let session = state.session()?;
    if mode > 0o7777 {
        return Err(AppError::invalid("Invalid permission bits"));
    }
    let targets = paths_arg(&paths)?;
    let flag = if recursive { "-R " } else { "" };
    run_op(&session, format!("chmod {flag}{mode:o} -- {targets}"), 300).await
}

#[tauri::command]
#[specta::specta]
pub async fn files_chown(state: State<'_, AppState>, paths: Vec<String>, owner: String, group: String, recursive: bool) -> AppResult<bool> {
    let session = state.session()?;
    let targets = paths_arg(&paths)?;
    let spec = match (owner.trim(), group.trim()) {
        ("", "") => return Err(AppError::invalid("Enter an owner or a group")),
        (o, "") => validate::username(o)?.to_string(),
        ("", g) => format!(":{}", validate::username(g)?),
        (o, g) => format!("{}:{}", validate::username(o)?, validate::username(g)?),
    };
    let flag = if recursive { "-R " } else { "" };
    // Changing ownership always needs root unless we already are.
    let exec = Exec::new(format!("chown {flag}{spec} -- {targets}")).secs(300);
    session.exec(exec.sudo()).await?.into_stdout()?;
    Ok(!session.is_root())
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileProperties {
    pub entry: FileEntry,
    pub link_target: Option<String>,
    pub mime: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn files_properties(state: State<'_, AppState>, path: String) -> AppResult<FileProperties> {
    let session = state.session()?;
    validate::abs_path(&path)?;
    let p = q(&path);
    let out = session
        .run_auto(format!(
            "stat -c '{STAT_FORMAT}' -- {p}; echo '#'; readlink -- {p} 2>/dev/null; echo '#'; file -b --mime-type -- {p} 2>/dev/null; true"
        ))
        .await?;
    let mut parts = out.split("#\n");
    let entry = parts
        .next()
        .and_then(|l| parse_stat_line(l.trim_end_matches('\n'), "/"))
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, path.clone()))?;
    let text = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Ok(FileProperties { entry, link_target: text(parts.next()), mime: text(parts.next()) })
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DirSize {
    pub path: String,
    pub bytes: u64,
}

pub fn parse_du(text: &str) -> Vec<DirSize> {
    text.lines()
        .filter_map(|line| {
            let (kb, path) = line.split_once('\t')?;
            Some(DirSize { bytes: kb.trim().parse::<u64>().ok()? * 1024, path: path.to_string() })
        })
        .collect()
}

/// Recursive sizes (disk usage) of the given paths.
#[tauri::command]
#[specta::specta]
pub async fn files_sizes(state: State<'_, AppState>, paths: Vec<String>) -> AppResult<Vec<DirSize>> {
    let session = state.session()?;
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut quoted = Vec::new();
    for p in &paths {
        quoted.push(q(validate::abs_path(p)?));
    }
    // `du` exits non-zero when it skips unreadable subtrees; the totals are still useful.
    let out = session.exec(Exec::new(format!("du -sk -- {} 2>/dev/null", quoted.join(" "))).secs(120)).await?;
    Ok(parse_du(&out.stdout))
}

/// Recursive case-insensitive name search below `root`.
#[tauri::command]
#[specta::specta]
pub async fn files_search(state: State<'_, AppState>, root: String, query: String) -> AppResult<Listing> {
    let session = state.session()?;
    validate::abs_path(&root)?;
    let query = query.trim();
    if query.is_empty() || query.contains('/') || query.contains('\n') {
        return Err(AppError::invalid("Enter part of a file name"));
    }
    // Escape glob metacharacters so the query is matched literally.
    let mut pattern = String::from("*");
    for c in query.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('*');
    let script = format!(
        "find {} -iname {} -exec stat -c '{STAT_FORMAT}' -- {{}} + 2>/dev/null | head -n {}",
        q(&root),
        q(&pattern),
        SEARCH_LIMIT + 1
    );
    let out = session.exec(Exec::new(script).secs(60)).await?;
    let mut entries: Vec<FileEntry> = out.stdout.lines().filter_map(|l| parse_stat_line(l, &root)).collect();
    let truncated = entries.len() > SEARCH_LIMIT;
    entries.truncate(SEARCH_LIMIT);
    Ok(Listing { path: root, entries, elevated: false, truncated })
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

/// Compress items of one directory into an archive in that directory.
#[tauri::command]
#[specta::specta]
pub async fn files_compress(
    state: State<'_, AppState>,
    dir: String,
    names: Vec<String>,
    archive_name: String,
    format: ArchiveFormat,
) -> AppResult<bool> {
    let session = state.session()?;
    validate::abs_path(&dir)?;
    validate::file_name(&archive_name)?;
    if names.is_empty() {
        return Err(AppError::invalid("Nothing selected"));
    }
    let mut items = Vec::new();
    for name in &names {
        items.push(q(&format!("./{}", validate::file_name(name)?)));
    }
    let items = items.join(" ");
    let archive = q(&archive_name);
    let pack = match format {
        ArchiveFormat::TarGz => format!("tar -czf {archive} -- {items}"),
        ArchiveFormat::Zip => {
            crate::deps::require(&session, crate::deps::Tool::Zip).await?;
            format!("zip -r -q {archive} {items}")
        }
    };
    let script =
        format!("cd {} && [ ! -e {archive} ] || {{ echo 'The archive already exists' >&2; exit 1; }}; cd {} && {pack}", q(&dir), q(&dir));
    run_op(&session, script, 1800).await
}

/// Extract an archive into the directory it is in.
#[tauri::command]
#[specta::specta]
pub async fn files_extract(state: State<'_, AppState>, path: String) -> AppResult<bool> {
    let session = state.session()?;
    validate::abs_path(&path)?;
    let lower = path.to_lowercase();
    let p = q(&path);
    let unpack = if lower.ends_with(".zip") {
        crate::deps::require(&session, crate::deps::Tool::Zip).await?;
        format!("unzip -q -o {p}")
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        format!("tar -xzf {p}")
    } else if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") {
        format!("tar -xjf {p}")
    } else if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        format!("tar -xJf {p}")
    } else if lower.ends_with(".tar") {
        format!("tar -xf {p}")
    } else if lower.ends_with(".gz") {
        format!("gunzip -k {p}")
    } else {
        return Err(AppError::unsupported("Unknown archive type"));
    };
    run_op(&session, format!("cd {} && {unpack}", q(&parent(&path))), 1800).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stat_lines() {
        let e = parse_stat_line("81a4\t1234\twww-data\twww-data\t1700000000\tindex with space.html", "/var/www").unwrap();
        assert_eq!(e.name, "index with space.html");
        assert_eq!(e.path, "/var/www/index with space.html");
        assert_eq!(e.kind, FileKind::File);
        assert_eq!(e.mode, 0o644);
        assert_eq!(e.size, 1234);
        assert_eq!(e.owner, "www-data");
        assert_eq!(e.modified, 1_700_000_000);

        let d = parse_stat_line("41ed\t4096\troot\troot\t1700000001\t.config", "/").unwrap();
        assert_eq!(d.kind, FileKind::Dir);
        assert!(d.is_dir_like);
        assert_eq!(d.mode, 0o755);
        assert_eq!(d.path, "/.config");

        let l = parse_stat_line("a1ff\t11\troot\troot\t1700000002\tlink", "/etc").unwrap();
        assert_eq!(l.kind, FileKind::Symlink);
        assert_eq!(l.mode, 0o777);

        let s = parse_stat_line("89ed\t0\troot\troot\t1\t/usr/bin/sudo", "/").unwrap();
        assert_eq!(s.mode, 0o4755);
        assert_eq!(s.name, "sudo");
        assert_eq!(s.path, "/usr/bin/sudo");

        assert!(parse_stat_line("garbage", "/").is_none());
        assert!(parse_stat_line("41ed\t1\ta\tb\t1\t.", "/").is_none());
    }

    #[test]
    fn listing_marks_directory_symlinks() {
        let out =
            "/srv\n41ed\t4096\troot\troot\t1\tapp\na1ff\t3\troot\troot\t1\tcurrent\na1ff\t3\troot\troot\t1\tfile-link\n#links\ncurrent\n";
        let l = parse_listing(out, true);
        assert_eq!(l.path, "/srv");
        assert!(l.elevated);
        assert_eq!(l.entries.len(), 3);
        assert!(l.entries[1].is_dir_like);
        assert!(!l.entries[2].is_dir_like);
    }

    #[test]
    fn path_helpers() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/etc", "nginx"), "/etc/nginx");
        assert_eq!(parent("/etc/nginx/nginx.conf"), "/etc/nginx");
        assert_eq!(parent("/etc/"), "/");
        assert_eq!(parent("/etc"), "/");
        assert_eq!(parent("/"), "/");
    }

    #[test]
    fn paths_arg_guards() {
        assert!(paths_arg(&[]).is_err());
        assert!(paths_arg(&["/".into()]).is_err());
        assert!(paths_arg(&["relative".into()]).is_err());
        assert_eq!(paths_arg(&["/a b".into(), "/c".into()]).unwrap(), "'/a b' /c");
    }

    #[test]
    fn list_script_quotes_the_directory() {
        let s = list_script("/tmp/it's here");
        assert!(s.contains("cd '/tmp/it'\\''s here'"));
    }

    #[test]
    fn parses_du() {
        let d = parse_du("12\t/var/log\n2048\t/home/a b\n");
        assert_eq!(d[0].bytes, 12 * 1024);
        assert_eq!(d[1].path, "/home/a b");
    }
}
