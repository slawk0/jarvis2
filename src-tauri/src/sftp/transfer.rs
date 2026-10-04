//! The transfer engine: a queue of upload / download / move / copy / delete
//! jobs with limited concurrency, progress events, retries and cancellation.
//!
//! Uploads and downloads stream through SFTP into a `.part` file, verify the
//! size and then replace the target atomically. When the login user lacks
//! permission they fall back to root through the sudo service.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use russh::ChannelMsg;
use russh_sftp::protocol::OpenFlags;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::files;
use crate::shell::{q, validate};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

const CONCURRENCY: usize = 3;
const MAX_ATTEMPTS: u32 = 3;
const CHUNK: usize = 64 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TransferKind {
    Upload,
    Download,
    Move,
    Copy,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    Overwrite,
    Skip,
    Rename,
}

/// One item the user asked to transfer. Directories are expanded into files.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    pub kind: TransferKind,
    /// Upload: local source. Download: local target. Unused otherwise.
    pub local: String,
    /// Upload: remote target. Download / move / copy / delete: remote source.
    pub remote: String,
    /// Move / copy: remote destination directory.
    pub destination: String,
    pub conflict: ConflictPolicy,
    /// Download only: delete the remote file afterwards (temporary exports).
    pub cleanup_remote: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TransferStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
    Skipped,
}

impl TransferStatus {
    fn finished(self) -> bool {
        !matches!(self, TransferStatus::Queued | TransferStatus::Running)
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TransferJob {
    pub id: String,
    pub batch_id: String,
    pub kind: TransferKind,
    pub name: String,
    pub local: String,
    pub remote: String,
    pub destination: String,
    pub size: u64,
    pub transferred: u64,
    /// Bytes per second over the last progress interval.
    #[specta(type = i32)]
    pub speed: f64,
    pub status: TransferStatus,
    pub error: Option<AppError>,
    pub attempts: u32,
    /// The transfer had to go through root.
    pub elevated: bool,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct TransferUpdate {
    pub job: TransferJob,
}

/// Every job of a batch reached a final state.
#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct TransferBatchDone {
    pub batch_id: String,
    pub failed: u32,
}

struct Slot {
    job: TransferJob,
    conflict: ConflictPolicy,
    cleanup_remote: bool,
    cancel: CancellationToken,
}

#[derive(Default)]
struct Inner {
    slots: Mutex<Vec<Slot>>,
    wake: Notify,
}

#[derive(Default)]
pub struct Transfers {
    inner: Arc<Inner>,
    pump_running: Mutex<bool>,
}

impl Transfers {
    pub fn cancel_all(&self) {
        let mut slots = self.inner.slots.lock();
        for slot in slots.iter_mut() {
            slot.cancel.cancel();
            if slot.job.status == TransferStatus::Queued {
                slot.job.status = TransferStatus::Cancelled;
            }
        }
    }

    fn snapshot(&self) -> Vec<TransferJob> {
        self.inner.slots.lock().iter().map(|s| s.job.clone()).collect()
    }
}

fn file_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// `name.ext` → `name (1).ext`, `name (2).ext`, …
pub fn numbered_name(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        _ => format!("{name} ({n})"),
    }
}

fn new_job(batch: &str, request: &TransferRequest, local: String, remote: String, size: u64) -> Slot {
    let name = match request.kind {
        TransferKind::Upload => file_name(&local),
        _ => file_name(&remote),
    };
    Slot {
        job: TransferJob {
            id: uuid::Uuid::new_v4().to_string(),
            batch_id: batch.to_string(),
            kind: request.kind,
            name,
            local,
            remote,
            destination: request.destination.clone(),
            size,
            transferred: 0,
            speed: 0.0,
            status: TransferStatus::Queued,
            error: None,
            attempts: 0,
            elevated: false,
        },
        conflict: request.conflict,
        cleanup_remote: request.cleanup_remote,
        cancel: CancellationToken::new(),
    }
}

/// Recursively collect files below a local directory as (path, size).
fn walk_local(dir: &Path, out: &mut Vec<(PathBuf, u64)>, dirs: &mut Vec<PathBuf>) -> std::io::Result<()> {
    dirs.push(dir.to_path_buf());
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            walk_local(&entry.path(), out, dirs)?;
        } else if meta.is_file() {
            out.push((entry.path(), meta.len()));
        }
    }
    Ok(())
}

fn to_remote_rel(base: &Path, path: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Turn one request into file-level jobs, creating directories as needed.
async fn expand(session: &Session, batch: &str, request: &TransferRequest) -> AppResult<Vec<Slot>> {
    match request.kind {
        TransferKind::Upload => {
            validate::abs_path(&request.remote)?;
            let local = PathBuf::from(&request.local);
            let meta = std::fs::metadata(&local)?;
            if !meta.is_dir() {
                return Ok(vec![new_job(batch, request, request.local.clone(), request.remote.clone(), meta.len())]);
            }
            let (mut found, mut dirs) = (Vec::new(), Vec::new());
            walk_local(&local, &mut found, &mut dirs)?;
            let remote_dirs: Vec<String> = dirs
                .iter()
                .map(|d| {
                    let rel = to_remote_rel(&local, d);
                    if rel.is_empty() {
                        q(&request.remote)
                    } else {
                        q(&files::join(&request.remote, &rel))
                    }
                })
                .collect();
            for chunk in remote_dirs.chunks(200) {
                session.exec_auto(Exec::new(format!("mkdir -p -- {}", chunk.join(" "))).secs(60)).await?.into_stdout()?;
            }
            Ok(found
                .into_iter()
                .map(|(path, size)| {
                    let remote = files::join(&request.remote, &to_remote_rel(&local, &path));
                    new_job(batch, request, path.to_string_lossy().into_owned(), remote, size)
                })
                .collect())
        }
        TransferKind::Download => {
            validate::abs_path(&request.remote)?;
            let r = q(&request.remote);
            // First line: "d" or "f"; then one "size<TAB>path" line per regular file.
            let script = format!(
                "if [ -d {r} ]; then echo d; find {r} -type f -exec stat -c '%s\t%n' -- {{}} +; \
                 else echo f; stat -c '%s\t%n' -- {r}; fi"
            );
            let out = session.exec_auto(Exec::new(script).secs(120)).await?.into_stdout()?;
            let mut lines = out.lines();
            let is_dir = lines.next() == Some("d");
            let base = request.remote.trim_end_matches('/');
            Ok(lines
                .filter_map(|line| {
                    let (size, path) = line.split_once('\t')?;
                    let local = if is_dir {
                        let rel = path.strip_prefix(base)?.trim_start_matches('/');
                        let mut target = PathBuf::from(&request.local);
                        target.extend(rel.split('/'));
                        target.to_string_lossy().into_owned()
                    } else {
                        request.local.clone()
                    };
                    Some(new_job(batch, request, local, path.to_string(), size.parse().ok()?))
                })
                .collect())
        }
        TransferKind::Move | TransferKind::Copy => {
            validate::abs_path(&request.remote)?;
            validate::abs_path(&request.destination)?;
            Ok(vec![new_job(batch, request, String::new(), request.remote.clone(), 0)])
        }
        TransferKind::Delete => {
            validate::abs_path(&request.remote)?;
            if request.remote.trim_end_matches('/').is_empty() {
                return Err(AppError::invalid("Refusing to delete /"));
            }
            Ok(vec![new_job(batch, request, String::new(), request.remote.clone(), 0)])
        }
    }
}

struct Runner {
    app: AppHandle,
    session: Arc<Session>,
    inner: Arc<Inner>,
    id: String,
    cancel: CancellationToken,
    last_emit: Instant,
    last_bytes: u64,
}

impl Runner {
    fn update(&self, edit: impl FnOnce(&mut TransferJob)) -> Option<TransferJob> {
        let mut slots = self.inner.slots.lock();
        let slot = slots.iter_mut().find(|s| s.job.id == self.id)?;
        edit(&mut slot.job);
        Some(slot.job.clone())
    }

    fn emit(&self, edit: impl FnOnce(&mut TransferJob)) {
        if let Some(job) = self.update(edit) {
            let _ = TransferUpdate { job }.emit(&self.app);
        }
    }

    fn progress(&mut self, transferred: u64) -> AppResult<()> {
        if self.cancel.is_cancelled() {
            return Err(AppError::code(ErrorCode::Cancelled));
        }
        let elapsed = self.last_emit.elapsed();
        if elapsed >= PROGRESS_INTERVAL {
            let speed = (transferred.saturating_sub(self.last_bytes)) as f64 / elapsed.as_secs_f64();
            self.last_emit = Instant::now();
            self.last_bytes = transferred;
            self.emit(|job| {
                job.transferred = transferred;
                job.speed = speed;
            });
        }
        Ok(())
    }

    async fn upload(&mut self, local: &str, remote: &str, size: u64) -> AppResult<()> {
        let sftp = self.session.sftp().await?;
        let part = format!("{remote}.part");
        let flags = OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE;
        let mut target = match sftp.open_with_flags(part.clone(), flags).await {
            Ok(file) => file,
            Err(e) => {
                let error = AppError::from(e);
                if error.is(ErrorCode::PermissionDenied) && !self.session.is_root() {
                    return self.upload_as_root(local, remote, size).await;
                }
                return Err(error);
            }
        };
        let mut source = tokio::fs::File::open(local).await?;
        let mut buffer = vec![0u8; CHUNK];
        let mut sent = 0u64;
        let copy = async {
            loop {
                let n = source.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                target.write_all(&buffer[..n]).await?;
                sent += n as u64;
                self.progress(sent)?;
            }
            target.shutdown().await?;
            Ok::<_, AppError>(())
        };
        if let Err(e) = copy.await {
            let _ = sftp.remove_file(part).await;
            return Err(e);
        }
        let written = sftp.metadata(part.clone()).await?.size.unwrap_or(0);
        if written != size {
            let _ = sftp.remove_file(part).await;
            return Err(AppError::new(ErrorCode::TransferFailed, format!("size mismatch: sent {size} bytes, server has {written}")));
        }
        // SFTP rename refuses to replace an existing file, so remove it first.
        let _ = sftp.remove_file(remote.to_string()).await;
        sftp.rename(part, remote.to_string()).await?;
        Ok(())
    }

    /// Upload into the user's cache directory, then move into place as root.
    async fn upload_as_root(&mut self, local: &str, remote: &str, size: u64) -> AppResult<()> {
        // Ask for the password (if needed) before sending any data.
        self.session.sudo_plan().await?;
        let staging_dir = format!("{}/.cache/jarvis", self.session.facts.home.trim_end_matches('/'));
        self.session.run(format!("mkdir -p -- {d} && chmod 700 {d}", d = q(&staging_dir))).await?;
        let staged = format!("{staging_dir}/upload-{}", uuid::Uuid::new_v4().simple());
        self.emit(|job| job.elevated = true);
        let sftp = self.session.sftp().await?;
        let result: AppResult<()> = async {
            let mut target = sftp.create(staged.clone()).await?;
            let mut source = tokio::fs::File::open(local).await?;
            let mut buffer = vec![0u8; CHUNK];
            let mut sent = 0u64;
            loop {
                let n = source.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                target.write_all(&buffer[..n]).await?;
                sent += n as u64;
                self.progress(sent)?;
            }
            target.shutdown().await?;
            if sent != size {
                return Err(AppError::new(ErrorCode::TransferFailed, "size mismatch"));
            }
            // New files get the directory's owner rather than the uploading user.
            self.session
                .run_sudo(format!(
                    "mv -f -- {s} {t} && chown --reference={dir} -- {t} 2>/dev/null; [ -e {t} ]",
                    s = q(&staged),
                    t = q(remote),
                    dir = q(&files::parent(remote))
                ))
                .await?;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = sftp.remove_file(staged).await;
        }
        result
    }

    async fn download(&mut self, remote: &str, local: &str, size: u64) -> AppResult<()> {
        let target = PathBuf::from(local);
        if let Some(dir) = target.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let mut part = target.clone().into_os_string();
        part.push(".part");
        let part = PathBuf::from(part);

        let sftp = self.session.sftp().await?;
        let result: AppResult<u64> = match sftp.open(remote.to_string()).await {
            Ok(mut source) => {
                let mut file = tokio::fs::File::create(&part).await?;
                let mut buffer = vec![0u8; CHUNK];
                let mut received = 0u64;
                async {
                    loop {
                        let n = source.read(&mut buffer).await?;
                        if n == 0 {
                            break;
                        }
                        file.write_all(&buffer[..n]).await?;
                        received += n as u64;
                        self.progress(received)?;
                    }
                    file.flush().await?;
                    file.sync_all().await?;
                    Ok(received)
                }
                .await
            }
            Err(e) => {
                let error = AppError::from(e);
                if error.is(ErrorCode::PermissionDenied) && !self.session.is_root() {
                    self.download_as_root(remote, &part).await
                } else {
                    Err(error)
                }
            }
        };
        let received = match result {
            Ok(n) => n,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e);
            }
        };
        if received != size {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(AppError::new(ErrorCode::TransferFailed, format!("size mismatch: expected {size} bytes, received {received}")));
        }
        // `rename` replaces an existing target on Unix; Windows needs it gone first.
        if cfg!(windows) && target.exists() {
            tokio::fs::remove_file(&target).await?;
        }
        tokio::fs::rename(&part, &target).await?;
        Ok(())
    }

    /// Stream a file the user cannot read through `sudo cat`.
    async fn download_as_root(&mut self, remote: &str, part: &Path) -> AppResult<u64> {
        self.emit(|job| job.elevated = true);
        let (line, stdin) = self.session.command_line(&format!("cat -- {}", q(remote)), true, None).await?;
        let mut lease = self.session.lease_stream().await?;
        lease.channel.exec(true, line).await?;
        if let Some(data) = &stdin {
            lease.channel.data(&data[..]).await?;
        }
        lease.channel.eof().await?;
        let mut file = tokio::fs::File::create(part).await?;
        let mut received = 0u64;
        let mut code = None;
        let mut stderr = Vec::new();
        while let Some(msg) = lease.channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    file.write_all(&data).await?;
                    received += data.len() as u64;
                    if let Err(e) = self.progress(received) {
                        let _ = lease.channel.close().await;
                        return Err(e);
                    }
                }
                ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                _ => {}
            }
        }
        file.flush().await?;
        file.sync_all().await?;
        if code != Some(0) {
            let (text, _) = crate::text::strip_tokens(&String::from_utf8_lossy(&stderr));
            return Err(AppError::new(ErrorCode::TransferFailed, text));
        }
        Ok(received)
    }

    async fn remote_op(&mut self, kind: TransferKind, remote: &str, destination: &str) -> AppResult<()> {
        let script = match kind {
            TransferKind::Move => format!("mv -- {} {}/", q(remote), q(destination.trim_end_matches('/'))),
            TransferKind::Copy => format!("cp -a -- {} {}/", q(remote), q(destination.trim_end_matches('/'))),
            _ => format!("rm -rf -- {}", q(remote)),
        };
        let exec = Exec::new(script).secs(3600);
        let cancel = self.cancel.clone();
        let session = self.session.clone();
        let work = async {
            if session.is_root() {
                return session.exec(exec).await?.into_stdout().map(|_| false);
            }
            let out = session.exec(exec.clone()).await?;
            if out.success() {
                Ok(false)
            } else if out.permission_denied() {
                session.exec(exec.sudo()).await?.into_stdout().map(|_| true)
            } else {
                Err(out.into_stdout().unwrap_err())
            }
        };
        let elevated = tokio::select! {
            result = work => result?,
            _ = cancel.cancelled() => return Err(AppError::code(ErrorCode::Cancelled)),
        };
        if elevated {
            self.emit(|job| job.elevated = true);
        }
        Ok(())
    }
}

/// Resolve the conflict policy for a job. `Ok(None)` means "skip it".
async fn resolve_target(session: &Session, job: &TransferJob, policy: ConflictPolicy) -> AppResult<Option<(String, String)>> {
    let mut local = job.local.clone();
    let mut remote = job.remote.clone();
    match job.kind {
        TransferKind::Upload if policy != ConflictPolicy::Overwrite => {
            let exists =
                |path: String| async move { session.exec_auto(Exec::new(format!("[ -e {} ]", q(&path)))).await.map(|o| o.success()) };
            if exists(remote.clone()).await? {
                if policy == ConflictPolicy::Skip {
                    return Ok(None);
                }
                let dir = files::parent(&remote);
                let name = file_name(&remote);
                for n in 1..1000 {
                    let candidate = files::join(&dir, &numbered_name(&name, n));
                    if !exists(candidate.clone()).await? {
                        remote = candidate;
                        break;
                    }
                }
            }
        }
        TransferKind::Download if policy != ConflictPolicy::Overwrite => {
            let target = PathBuf::from(&local);
            if target.exists() {
                if policy == ConflictPolicy::Skip {
                    return Ok(None);
                }
                let name = file_name(&local);
                for n in 1..1000 {
                    let candidate = target.with_file_name(numbered_name(&name, n));
                    if !candidate.exists() {
                        local = candidate.to_string_lossy().into_owned();
                        break;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(Some((local, remote)))
}

async fn run_job(app: AppHandle, session: Arc<Session>, inner: Arc<Inner>, id: String) {
    let Some((job, policy, cleanup, cancel)) = ({
        let slots = inner.slots.lock();
        slots.iter().find(|s| s.job.id == id).map(|s| (s.job.clone(), s.conflict, s.cleanup_remote, s.cancel.clone()))
    }) else {
        return;
    };
    let mut runner = Runner { app, session: session.clone(), inner: inner.clone(), id, cancel, last_emit: Instant::now(), last_bytes: 0 };

    let outcome: AppResult<TransferStatus> = async {
        let Some((local, remote)) = resolve_target(&session, &job, policy).await? else {
            return Ok(TransferStatus::Skipped);
        };
        runner.emit(|j| {
            j.local = local.clone();
            j.remote = remote.clone();
        });
        let mut attempt = 0;
        loop {
            attempt += 1;
            runner.last_bytes = 0;
            runner.emit(|j| {
                j.attempts = attempt;
                j.transferred = 0;
            });
            let result = match job.kind {
                TransferKind::Upload => runner.upload(&local, &remote, job.size).await,
                TransferKind::Download => runner.download(&remote, &local, job.size).await,
                kind => runner.remote_op(kind, &remote, &job.destination).await,
            };
            match result {
                Ok(()) => break,
                Err(e) => {
                    let transient =
                        matches!(e.code, ErrorCode::ConnectionLost | ErrorCode::Timeout | ErrorCode::TransferFailed | ErrorCode::Sftp);
                    if !transient || attempt >= MAX_ATTEMPTS || runner.cancel.is_cancelled() {
                        return Err(e);
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(1 << attempt)) => {}
                        _ = runner.cancel.cancelled() => return Err(AppError::code(ErrorCode::Cancelled)),
                    }
                }
            }
        }
        if cleanup && job.kind == TransferKind::Download {
            let _ = session.exec_auto(Exec::new(format!("rm -f -- {}", q(&remote)))).await;
        }
        Ok(TransferStatus::Done)
    }
    .await;

    runner.emit(|j| {
        j.speed = 0.0;
        match outcome {
            Ok(status) => {
                j.status = status;
                if status == TransferStatus::Done {
                    j.transferred = j.size;
                }
            }
            Err(e) if e.is(ErrorCode::Cancelled) => j.status = TransferStatus::Cancelled,
            Err(e) => {
                j.status = TransferStatus::Failed;
                j.error = Some(e);
            }
        }
    });

    // Report the batch once nothing in it is still pending.
    let done = {
        let slots = inner.slots.lock();
        let batch: Vec<&Slot> = slots.iter().filter(|s| s.job.batch_id == job.batch_id).collect();
        batch
            .iter()
            .all(|s| s.job.status.finished())
            .then(|| batch.iter().filter(|s| s.job.status == TransferStatus::Failed).count() as u32)
    };
    if let Some(failed) = done {
        let _ = TransferBatchDone { batch_id: job.batch_id, failed }.emit(&runner.app);
    }
    inner.wake.notify_one();
}

/// Starts queued jobs, at most `CONCURRENCY` at a time, until the queue is empty.
async fn pump(app: AppHandle, session: Arc<Session>, inner: Arc<Inner>) {
    loop {
        let next: Vec<String> = {
            let mut slots = inner.slots.lock();
            let running = slots.iter().filter(|s| s.job.status == TransferStatus::Running).count();
            let mut picked = Vec::new();
            for slot in slots.iter_mut() {
                if running + picked.len() >= CONCURRENCY {
                    break;
                }
                if slot.job.status == TransferStatus::Queued {
                    slot.job.status = TransferStatus::Running;
                    picked.push(slot.job.id.clone());
                }
            }
            picked
        };
        for id in next {
            tauri::async_runtime::spawn(run_job(app.clone(), session.clone(), inner.clone(), id));
        }
        let idle = {
            let slots = inner.slots.lock();
            !slots.iter().any(|s| matches!(s.job.status, TransferStatus::Queued | TransferStatus::Running))
        };
        if idle {
            return;
        }
        tokio::select! {
            _ = inner.wake.notified() => {}
            _ = session.shutdown.cancelled() => return,
        }
    }
}

fn start_pump(app: &AppHandle, state: &AppState, session: Arc<Session>) {
    let mut running = state.transfers.pump_running.lock();
    if *running {
        state.transfers.inner.wake.notify_one();
        return;
    }
    *running = true;
    let app = app.clone();
    let inner = state.transfers.inner.clone();
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        loop {
            pump(app.clone(), session.clone(), inner.clone()).await;
            // Re-check under the flag's lock so a job queued right now is not stranded.
            let state = app.state::<AppState>();
            let mut running = state.transfers.pump_running.lock();
            let pending = inner.slots.lock().iter().any(|s| s.job.status == TransferStatus::Queued);
            if !pending || session.shutdown.is_cancelled() {
                *running = false;
                return;
            }
        }
    });
}

// ---------------------------------------------------------------- commands

/// Queue a batch for `session`. Batches are appended to the queue, never rejected.
pub async fn enqueue(app: &AppHandle, state: &AppState, session: Arc<Session>, requests: Vec<TransferRequest>) -> AppResult<String> {
    let batch = uuid::Uuid::new_v4().to_string();
    let mut slots = Vec::new();
    for request in &requests {
        slots.extend(expand(&session, &batch, request).await?);
    }
    if slots.is_empty() {
        return Err(AppError::invalid("Nothing to transfer"));
    }
    for slot in &slots {
        let _ = TransferUpdate { job: slot.job.clone() }.emit(app);
    }
    state.transfers.inner.slots.lock().extend(slots);
    start_pump(app, state, session);
    Ok(batch)
}

/// Queue a batch. Batches are appended to the queue, never rejected.
#[tauri::command]
#[specta::specta]
pub async fn transfer_enqueue(app: AppHandle, state: State<'_, AppState>, requests: Vec<TransferRequest>) -> AppResult<String> {
    let session = state.session()?;
    enqueue(&app, &state, session, requests).await
}

/// Which of the requested targets already exist (drives the conflict dialog).
#[tauri::command]
#[specta::specta]
pub async fn transfer_conflicts(state: State<'_, AppState>, requests: Vec<TransferRequest>) -> AppResult<Vec<String>> {
    let session = state.session()?;
    let mut existing = Vec::new();
    let mut remote_checks = Vec::new();
    for request in &requests {
        match request.kind {
            TransferKind::Upload => {
                validate::abs_path(&request.remote)?;
                remote_checks.push(request.remote.clone());
            }
            TransferKind::Download => {
                if Path::new(&request.local).exists() {
                    existing.push(request.local.clone());
                }
            }
            TransferKind::Move | TransferKind::Copy => {
                validate::abs_path(&request.destination)?;
                remote_checks.push(files::join(&request.destination, &file_name(&request.remote)));
            }
            TransferKind::Delete => {}
        }
    }
    if !remote_checks.is_empty() {
        let script =
            remote_checks.iter().enumerate().map(|(i, path)| format!("[ -e {} ] && echo {i}", q(path))).collect::<Vec<_>>().join("; ");
        let out = session.exec_auto(Exec::new(format!("{script}; true"))).await?;
        for line in out.stdout.lines() {
            if let Some(path) = line.trim().parse::<usize>().ok().and_then(|i| remote_checks.get(i)) {
                existing.push(path.clone());
            }
        }
    }
    Ok(existing)
}

#[tauri::command]
#[specta::specta]
pub fn transfer_list(state: State<'_, AppState>) -> Vec<TransferJob> {
    state.transfers.snapshot()
}

/// Cancel one job, one batch, or (with neither) everything.
#[tauri::command]
#[specta::specta]
pub fn transfer_cancel(app: AppHandle, state: State<'_, AppState>, job_id: Option<String>, batch_id: Option<String>) {
    let mut slots = state.transfers.inner.slots.lock();
    for slot in slots.iter_mut() {
        let selected = match (&job_id, &batch_id) {
            (Some(id), _) => &slot.job.id == id,
            (None, Some(batch)) => &slot.job.batch_id == batch,
            (None, None) => true,
        };
        if !selected || slot.job.status.finished() {
            continue;
        }
        slot.cancel.cancel();
        if slot.job.status == TransferStatus::Queued {
            slot.job.status = TransferStatus::Cancelled;
            let _ = TransferUpdate { job: slot.job.clone() }.emit(&app);
        }
    }
    drop(slots);
    state.transfers.inner.wake.notify_one();
}

/// Put failed and cancelled jobs back in the queue (all of them, or the given ones).
#[tauri::command]
#[specta::specta]
pub fn transfer_retry_failed(app: AppHandle, state: State<'_, AppState>, job_ids: Option<Vec<String>>) -> AppResult<()> {
    let session = state.session()?;
    {
        let mut slots = state.transfers.inner.slots.lock();
        for slot in slots.iter_mut() {
            let selected = job_ids.as_ref().is_none_or(|ids| ids.contains(&slot.job.id));
            if selected && matches!(slot.job.status, TransferStatus::Failed | TransferStatus::Cancelled) {
                slot.job.status = TransferStatus::Queued;
                slot.job.error = None;
                slot.job.transferred = 0;
                slot.cancel = CancellationToken::new();
                let _ = TransferUpdate { job: slot.job.clone() }.emit(&app);
            }
        }
    }
    start_pump(&app, &state, session);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn transfer_clear_completed(state: State<'_, AppState>) -> Vec<TransferJob> {
    state.transfers.inner.slots.lock().retain(|s| !s.job.status.finished());
    state.transfers.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_names_keep_the_extension() {
        assert_eq!(numbered_name("report.pdf", 1), "report (1).pdf");
        assert_eq!(numbered_name("archive.tar.gz", 2), "archive.tar (2).gz");
        assert_eq!(numbered_name("Makefile", 3), "Makefile (3)");
        assert_eq!(numbered_name(".env", 1), ".env (1)");
    }

    #[test]
    fn file_names_from_either_path_style() {
        assert_eq!(file_name("/var/log/syslog"), "syslog");
        assert_eq!(file_name("C:\\Users\\me\\a.txt"), "a.txt");
        assert_eq!(file_name("/srv/site/"), "site");
    }

    #[test]
    fn relative_remote_paths_use_forward_slashes() {
        let base = Path::new("root").join("site");
        let file = base.join("css").join("app.css");
        assert_eq!(to_remote_rel(&base, &file), "css/app.css");
        assert_eq!(to_remote_rel(&base, &base), "");
    }

    #[test]
    fn walks_local_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("top.txt"), b"12345").unwrap();
        std::fs::write(dir.path().join("a/b/deep.txt"), b"1").unwrap();
        let (mut files, mut dirs) = (Vec::new(), Vec::new());
        walk_local(dir.path(), &mut files, &mut dirs).unwrap();
        files.sort();
        assert_eq!(files.len(), 2);
        assert_eq!(dirs.len(), 3);
        assert!(files.iter().any(|(p, s)| p.ends_with("top.txt") && *s == 5));
    }
}
