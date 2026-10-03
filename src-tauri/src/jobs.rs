//! The one mechanism for long-running, streamed operations.
//!
//! Starting a job returns a `jobId`; progress arrives as [`JobOutput`] events
//! and the result as a single [`JobDone`]. Cancelling terminates the remote
//! process group, not just the local listener.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use parking_lot::Mutex;
use russh::ChannelMsg;
use serde::Serialize;
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::ssh::session::{Exec, Session};
use crate::text::{TokenFilter, Utf8Chunker};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum JobStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct JobStarted {
    pub job_id: String,
    /// Short label, e.g. "Compose up · blog".
    pub title: String,
    /// Full command or description.
    pub detail: String,
    /// Whether the job belongs in the Running Jobs panel.
    pub visible: bool,
    pub started_at: i64,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct JobOutput {
    pub job_id: String,
    pub stream: JobStream,
    pub chunk: String,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct JobDone {
    pub job_id: String,
    pub exit_code: Option<i32>,
    pub error: Option<AppError>,
    pub cancelled: bool,
}

#[derive(Debug, Clone)]
pub struct JobMeta {
    pub title: String,
    pub detail: String,
    pub visible: bool,
}

impl JobMeta {
    /// A job listed in the Running Jobs panel.
    pub fn visible(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            detail: detail.into(),
            visible: true,
        }
    }

    /// A stream only its own view cares about (log follow, live stats…).
    pub fn hidden(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            detail: String::new(),
            visible: false,
        }
    }
}

/// Handed to the body of a job: output sink, cancellation and remote streaming.
#[derive(Clone)]
pub struct JobCtx {
    pub id: String,
    pub app: AppHandle,
    pub cancel: CancellationToken,
    pub session: Arc<Session>,
}

impl JobCtx {
    pub fn emit(&self, stream: JobStream, chunk: impl Into<String>) {
        let chunk = chunk.into();
        if chunk.is_empty() {
            return;
        }
        let _ = JobOutput {
            job_id: self.id.clone(),
            stream,
            chunk,
        }
        .emit(&self.app);
    }

    /// Write a line of the job's own commentary to the log.
    pub fn note(&self, line: impl AsRef<str>) {
        self.emit(JobStream::Stdout, format!("{}\n", line.as_ref()));
    }

    pub fn check_cancelled(&self) -> AppResult<()> {
        if self.cancel.is_cancelled() {
            Err(AppError::code(ErrorCode::Cancelled))
        } else {
            Ok(())
        }
    }

    /// Run a remote command, streaming its output into the job log, and
    /// return its exit code. `exec.timeout` is ignored: jobs run until they
    /// finish or are stopped.
    pub async fn stream(&self, exec: Exec) -> AppResult<i32> {
        self.stream_with(exec, |_| {}).await
    }

    /// Like [`stream`](Self::stream), also handing every stdout chunk to `tap`.
    pub async fn stream_with(
        &self,
        exec: Exec,
        mut tap: impl FnMut(&str) + Send,
    ) -> AppResult<i32> {
        self.check_cancelled()?;
        let session = &self.session;
        // The inner shell reports its PID so a cancel can signal the process group.
        let script = format!("echo \"[[jarvis:pid:$$]]\" >&2; {}", exec.script);
        let (line, stdin) = session.command_line(&script, exec.sudo, exec.stdin).await?;
        let mut lease = session.lease_stream().await?;
        lease.channel.exec(true, line).await?;
        if let Some(data) = &stdin {
            lease.channel.data(&data[..]).await?;
        }
        lease.channel.eof().await?;

        let mut out = Utf8Chunker::new();
        let mut err = Utf8Chunker::new();
        let mut filter = TokenFilter::new();
        let mut pid: Option<u32> = None;
        let mut sudo_prompts = 0;
        let mut code = None;

        loop {
            tokio::select! {
                msg = lease.channel.wait() => {
                    let Some(msg) = msg else { break };
                    match msg {
                        ChannelMsg::Data { data } => {
                            let text = out.push(&data);
                            tap(&text);
                            self.emit(JobStream::Stdout, text);
                        }
                        ChannelMsg::ExtendedData { data, ext: 1 } => {
                            let (text, tokens) = filter.push(&err.push(&data));
                            for token in tokens {
                                if token == "sudo" {
                                    sudo_prompts += 1;
                                } else if let Some(p) = token.strip_prefix("pid:") {
                                    pid = p.parse().ok();
                                }
                            }
                            self.emit(JobStream::Stderr, text);
                        }
                        ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status as i32),
                        ChannelMsg::ExitSignal { .. } => code = code.or(Some(-1)),
                        _ => {}
                    }
                }
                _ = self.cancel.cancelled() => {
                    if let Some(pid) = pid {
                        kill_remote(session, pid, exec.sudo).await;
                    }
                    let _ = lease.channel.close().await;
                    return Err(AppError::code(ErrorCode::Cancelled));
                }
                _ = session.shutdown.cancelled() => {
                    return Err(AppError::code(ErrorCode::NotConnected));
                }
            }
        }

        let tail = out.finish();
        tap(&tail);
        self.emit(JobStream::Stdout, tail);
        let (text, _) = filter.push(&err.finish());
        self.emit(JobStream::Stderr, format!("{text}{}", filter.finish()));

        if exec.sudo && sudo_prompts > 1 {
            session.sudo.invalidate();
            return Err(AppError::code(ErrorCode::SudoPasswordRequired));
        }
        code.ok_or_else(|| {
            AppError::new(
                ErrorCode::ConnectionLost,
                "channel closed before the command finished",
            )
        })
    }

    /// Stream a command and fail the job if it exits non-zero.
    pub async fn step(&self, exec: Exec) -> AppResult<()> {
        match self.stream(exec).await? {
            0 => Ok(()),
            code => Err(AppError::new(
                ErrorCode::CommandFailed,
                format!("exit code {code}"),
            )),
        }
    }
}

/// Terminate the remote process group started by a job (TERM, then KILL).
async fn kill_remote(session: &Session, pid: u32, sudo: bool) {
    let script = format!(
        "pg=$(ps -o pgid= -p {pid} 2>/dev/null | tr -d ' '); \
         if [ -n \"$pg\" ] && [ \"$pg\" -gt 1 ] 2>/dev/null; then \
           kill -TERM -- \"-$pg\" 2>/dev/null; sleep 3; kill -KILL -- \"-$pg\" 2>/dev/null; \
         else \
           pkill -TERM -P {pid} 2>/dev/null; kill -TERM {pid} 2>/dev/null; sleep 3; \
           pkill -KILL -P {pid} 2>/dev/null; kill -KILL {pid} 2>/dev/null; \
         fi; true"
    );
    // If the sudo password expired meanwhile there is nothing more we can do here;
    // closing the channel still tears down the local side.
    let _ = session.exec(Exec::new(script).sudo_if(sudo).secs(15)).await;
}

#[derive(Default)]
pub struct Jobs {
    running: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl Jobs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `body` as a job. The body returns the exit code to report.
    pub fn spawn<F, Fut>(
        &self,
        app: &AppHandle,
        session: Arc<Session>,
        meta: JobMeta,
        body: F,
    ) -> String
    where
        F: FnOnce(JobCtx) -> Fut + Send + 'static,
        Fut: Future<Output = AppResult<i32>> + Send + 'static,
    {
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = CancellationToken::new();
        self.running.lock().insert(id.clone(), cancel.clone());
        let ctx = JobCtx {
            id: id.clone(),
            app: app.clone(),
            cancel,
            session,
        };
        let _ = JobStarted {
            job_id: id.clone(),
            title: meta.title,
            detail: meta.detail,
            visible: meta.visible,
            started_at: chrono::Utc::now().timestamp_millis(),
        }
        .emit(app);

        let running = self.running.clone();
        tauri::async_runtime::spawn(async move {
            let result = body(ctx.clone()).await;
            running.lock().remove(&ctx.id);
            let done = match result {
                Ok(code) => JobDone {
                    job_id: ctx.id.clone(),
                    exit_code: Some(code),
                    error: None,
                    cancelled: false,
                },
                Err(e) => JobDone {
                    job_id: ctx.id.clone(),
                    exit_code: None,
                    cancelled: e.is(ErrorCode::Cancelled),
                    error: Some(e),
                },
            };
            let _ = done.emit(&ctx.app);
        });
        id
    }

    /// Start a single streamed remote command as a job. Elevation problems
    /// surface here, before the job exists, so the sudo dialog can retry.
    pub async fn start(
        &self,
        app: &AppHandle,
        session: Arc<Session>,
        meta: JobMeta,
        exec: Exec,
    ) -> AppResult<String> {
        if exec.sudo {
            session.sudo_plan().await?;
        }
        Ok(self.spawn(app, session, meta, move |ctx| async move {
            ctx.stream(exec).await
        }))
    }

    pub fn cancel(&self, job_id: &str) {
        if let Some(token) = self.running.lock().get(job_id) {
            token.cancel();
        }
    }

    pub fn cancel_all(&self) {
        for token in self.running.lock().values() {
            token.cancel();
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn job_cancel(state: tauri::State<'_, crate::state::AppState>, job_id: String) {
    state.jobs.cancel(&job_id);
}
