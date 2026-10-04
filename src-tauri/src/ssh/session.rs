//! The active server session: connections, command execution and server facts.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use russh::client::Msg;
use russh::{Channel, ChannelMsg};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use super::client::{self, ConnectFailure, Handle, Target};
use super::known_hosts::KnownHosts;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::shell::q;
use crate::store::profiles::Profile;
use crate::sudo::{Plan, Sudo};
use crate::text::strip_tokens;

/// OpenSSH's default `MaxSessions` is 10; stay below it per connection.
const CHANNELS_PER_CONNECTION: usize = 6;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT: usize = 64 * 1024 * 1024;

/// Non-login exec shells of regular users often lack the sbin directories.
const PATH_FIX: &str = "PATH=\"$PATH:/usr/local/sbin:/usr/sbin:/sbin\"; export PATH; ";
pub const SUDO_PROMPT: &str = "[[jarvis:sudo]]";

pub struct Conn {
    handle: Handle,
    slots: Arc<Semaphore>,
}

impl Conn {
    fn new(handle: Handle) -> Arc<Self> {
        Arc::new(Self { handle, slots: Arc::new(Semaphore::new(CHANNELS_PER_CONNECTION)) })
    }

    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// A session channel plus the slot and connection keeping it alive.
pub struct Lease {
    pub channel: Channel<Msg>,
    _permit: Option<OwnedSemaphorePermit>,
    pub conn: Arc<Conn>,
}

/// The shared SFTP subsystem plus what keeps its channel alive.
struct SftpHandle {
    client: Arc<russh_sftp::client::SftpSession>,
    conn: Arc<Conn>,
    _permit: Option<OwnedSemaphorePermit>,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub uid: u32,
    pub user: String,
    pub home: String,
    pub has_sudo: bool,
}

#[derive(Debug, Clone)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == 0
    }

    pub fn permission_denied(&self) -> bool {
        looks_like_permission_denied(&self.stderr) || looks_like_permission_denied(&self.stdout)
    }

    /// The most useful text to show when the command failed.
    pub fn failure_text(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return tail(stderr, 4000);
        }
        let stdout = self.stdout.trim();
        if !stdout.is_empty() {
            return tail(stdout, 4000);
        }
        format!("exit code {}", self.code)
    }

    pub fn into_stdout(self) -> AppResult<String> {
        if self.success() {
            return Ok(self.stdout);
        }
        let code = if self.code == 127 {
            ErrorCode::DependencyMissing
        } else if self.permission_denied() {
            ErrorCode::PermissionDenied
        } else {
            ErrorCode::CommandFailed
        };
        Err(AppError::new(code, self.failure_text()))
    }

    /// stdout followed by stderr, for "show me what happened" views.
    pub fn combined(&self) -> String {
        let mut text = self.stdout.clone();
        if !self.stderr.trim().is_empty() {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&self.stderr);
        }
        text
    }
}

fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}

/// Recognise "you are not allowed" in `LC_ALL=C` tool output.
pub fn looks_like_permission_denied(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "permission denied",
        "operation not permitted",
        "must be root",
        "are you root",
        "requires root",
        "need to be root",
        "must be run as root",
        "access denied",
        "superuser privileges",
        "not permitted",
        "authentication is required",
        "interactive authentication required",
    ]
    .iter()
    .any(|needle| t.contains(needle))
}

/// One remote command: a POSIX shell script plus how to run it.
#[derive(Debug, Clone)]
pub struct Exec {
    pub script: String,
    pub sudo: bool,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
}

impl Exec {
    pub fn new(script: impl Into<String>) -> Self {
        Self { script: script.into(), sudo: false, stdin: None, timeout: DEFAULT_TIMEOUT }
    }

    pub fn sudo(mut self) -> Self {
        self.sudo = true;
        self
    }

    pub fn sudo_if(mut self, sudo: bool) -> Self {
        self.sudo = sudo;
        self
    }

    pub fn stdin(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(data.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn secs(self, secs: u64) -> Self {
        self.timeout(Duration::from_secs(secs))
    }
}

pub struct Session {
    pub profile: Profile,
    target: Target,
    known_hosts: Arc<KnownHosts>,
    main: RwLock<Arc<Conn>>,
    streams: Mutex<Vec<Arc<Conn>>>,
    pub sudo: Sudo,
    pub facts: Facts,
    /// Cancelled when the session is torn down; everything bound to it stops.
    pub shutdown: CancellationToken,
    /// Lazily detected: does `docker` need sudo on this server?
    pub docker_sudo: tokio::sync::Mutex<Option<bool>>,
    sftp: tokio::sync::Mutex<Option<SftpHandle>>,
}

impl Session {
    pub async fn open(profile: Profile, target: Target, known_hosts: Arc<KnownHosts>) -> Result<Arc<Self>, ConnectFailure> {
        let handle = client::connect(&target, known_hosts.clone()).await?;
        let mut session = Self {
            profile,
            target,
            known_hosts,
            main: RwLock::new(Conn::new(handle)),
            streams: Mutex::new(Vec::new()),
            sudo: Sudo::new(),
            facts: Facts::default(),
            shutdown: CancellationToken::new(),
            docker_sudo: tokio::sync::Mutex::new(None),
            sftp: tokio::sync::Mutex::new(None),
        };
        session.facts = session.detect_facts().await?;
        Ok(Arc::new(session))
    }

    async fn detect_facts(&self) -> AppResult<Facts> {
        let out = self.run("id -u; id -un; printf '%s\\n' \"$HOME\"; command -v sudo >/dev/null 2>&1 && echo yes || echo no").await?;
        let mut lines = out.lines();
        let mut next = || lines.next().unwrap_or("").trim().to_string();
        let uid = next().parse().unwrap_or(u32::MAX);
        let user = next();
        let home = next();
        let has_sudo = next() == "yes";
        Ok(Facts { uid, user, home: if home.is_empty() { "/".into() } else { home }, has_sudo })
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn is_root(&self) -> bool {
        self.facts.uid == 0
    }

    fn main(&self) -> Arc<Conn> {
        self.main.read().clone()
    }

    pub fn main_closed(&self) -> bool {
        self.main().is_closed()
    }

    /// Replace the main connection after it was lost. Sessions, terminals and
    /// streams that lived on dead connections end on their own.
    pub async fn reconnect(&self) -> AppResult<()> {
        let handle = client::connect(&self.target, self.known_hosts.clone()).await?;
        *self.main.write() = Conn::new(handle);
        self.streams.lock().retain(|c| !c.is_closed());
        *self.sftp.lock().await = None;
        Ok(())
    }

    /// A brand-new authenticated connection (terminals get one each).
    pub async fn dedicated(&self) -> AppResult<Arc<Conn>> {
        let handle = client::connect(&self.target, self.known_hosts.clone()).await?;
        Ok(Conn::new(handle))
    }

    async fn open_channel(conn: Arc<Conn>, permit: Option<OwnedSemaphorePermit>) -> AppResult<Lease> {
        let mut attempt = 0u64;
        let channel = loop {
            let opening = conn.handle.channel_open_session();
            let result = tokio::time::timeout(Duration::from_secs(15), opening)
                .await
                .map_err(|_| AppError::new(ErrorCode::Timeout, "opening SSH channel"))?;
            match result {
                Ok(channel) => break channel,
                // sshd still counts sessions that are closing against MaxSessions,
                // so a refusal right after other commands ended is transient.
                Err(russh::Error::ChannelOpenFailure(_)) if attempt < 8 && !conn.is_closed() => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(100 * attempt)).await;
                }
                Err(e) if conn.is_closed() => return Err(AppError::new(ErrorCode::ConnectionLost, e.to_string())),
                Err(e) => return Err(e.into()),
            }
        };
        Ok(Lease { channel, _permit: permit, conn })
    }

    /// Channel on the main connection, for short request/response commands.
    pub async fn lease(&self) -> AppResult<Lease> {
        let conn = self.main();
        if conn.is_closed() {
            return Err(AppError::code(ErrorCode::ConnectionLost));
        }
        let permit = conn.slots.clone().acquire_owned().await.map_err(|_| AppError::code(ErrorCode::ConnectionLost))?;
        Self::open_channel(conn, Some(permit)).await
    }

    /// Channel for a long-lived stream (job, log follow, transfer). These use
    /// pooled side connections so they never starve short commands.
    pub async fn lease_stream(&self) -> AppResult<Lease> {
        let existing = {
            let mut pool = self.streams.lock();
            pool.retain(|c| !c.is_closed());
            pool.iter().find_map(|c| c.slots.clone().try_acquire_owned().ok().map(|p| (c.clone(), p)))
        };
        let (conn, permit) = match existing {
            Some(found) => found,
            None => {
                let conn = self.dedicated().await?;
                let permit = conn.slots.clone().try_acquire_owned().map_err(|_| AppError::internal("fresh connection has no free slot"))?;
                self.streams.lock().push(conn.clone());
                (conn, permit)
            }
        };
        Self::open_channel(conn, Some(permit)).await
    }

    /// The session's SFTP client, opened on first use on a side connection.
    pub async fn sftp(&self) -> AppResult<Arc<russh_sftp::client::SftpSession>> {
        let mut slot = self.sftp.lock().await;
        if let Some(handle) = slot.as_ref() {
            if !handle.conn.is_closed() {
                return Ok(handle.client.clone());
            }
        }
        let lease = self.lease_stream().await?;
        lease.channel.request_subsystem(true, "sftp").await?;
        let Lease { channel, _permit, conn } = lease;
        let client = russh_sftp::client::SftpSession::new(channel.into_stream()).await?;
        client.set_timeout(60);
        let client = Arc::new(client);
        *slot = Some(SftpHandle { client: client.clone(), conn, _permit });
        Ok(client)
    }

    /// Open a `direct-tcpip` channel to `host:port` as seen from the server.
    pub async fn tunnel(&self, host: &str, port: u16) -> AppResult<Channel<Msg>> {
        let conn = {
            let mut pool = self.streams.lock();
            pool.retain(|c| !c.is_closed());
            pool.first().cloned()
        };
        let conn = match conn {
            Some(c) => c,
            None => {
                let c = self.dedicated().await?;
                self.streams.lock().push(c.clone());
                c
            }
        };
        Ok(conn.handle.channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0).await?)
    }

    /// Wrap a script for the remote login shell: fixed locale, POSIX `sh`.
    fn user_line(script: &str) -> String {
        format!("env LC_ALL=C LANG=C sh -c {}", q(&format!("{PATH_FIX}{script}")))
    }

    /// Final command line and stdin for a script, applying elevation.
    /// The sudo password only ever travels through the channel's stdin.
    pub async fn command_line(&self, script: &str, sudo: bool, stdin: Option<Vec<u8>>) -> AppResult<(String, Option<Vec<u8>>)> {
        let line = Self::user_line(script);
        if !sudo {
            return Ok((line, stdin));
        }
        match self.sudo_plan().await? {
            Plan::Direct => Ok((line, stdin)),
            Plan::NoPassword => Ok((format!("sudo -n -- {line}"), stdin)),
            Plan::WithPassword(password) => {
                let mut input = password.into_bytes();
                input.push(b'\n');
                input.extend(stdin.unwrap_or_default());
                Ok((format!("sudo -k -S -p {} -- {line}", q(SUDO_PROMPT)), Some(input)))
            }
        }
    }

    /// Run an exact command line on the main connection and collect its output.
    pub async fn exec_line(&self, line: &str, stdin: Option<&[u8]>, timeout: Duration) -> AppResult<Output> {
        let mut lease = self.lease().await?;
        let run = collect(&mut lease.channel, line, stdin);
        tokio::select! {
            result = tokio::time::timeout(timeout, run) => match result {
                Ok(output) => output,
                Err(_) => {
                    let _ = lease.channel.close().await;
                    Err(AppError::new(ErrorCode::Timeout, format!("no result after {}s", timeout.as_secs())))
                }
            },
            _ = self.shutdown.cancelled() => {
                let _ = lease.channel.close().await;
                Err(AppError::code(ErrorCode::NotConnected))
            }
        }
    }

    pub async fn exec(&self, exec: Exec) -> AppResult<Output> {
        let (line, stdin) = self.command_line(&exec.script, exec.sudo, exec.stdin).await?;
        let mut output = self.exec_line(&line, stdin.as_deref(), exec.timeout).await?;
        if exec.sudo {
            let (stderr, tokens) = strip_tokens(&output.stderr);
            // sudo prompts once for a correct password; a second prompt means it was rejected.
            if tokens.iter().filter(|t| *t == "sudo").count() > 1 {
                self.sudo.invalidate();
                return Err(AppError::code(ErrorCode::SudoPasswordRequired));
            }
            output.stderr = stderr;
        }
        Ok(output)
    }

    /// Run as the login user; non-zero exit is an error.
    pub async fn run(&self, script: impl Into<String>) -> AppResult<String> {
        self.exec(Exec::new(script)).await?.into_stdout()
    }

    /// Run as root through the sudo service; non-zero exit is an error.
    pub async fn run_sudo(&self, script: impl Into<String>) -> AppResult<String> {
        self.exec(Exec::new(script).sudo()).await?.into_stdout()
    }

    pub async fn run_as(&self, script: impl Into<String>, sudo: bool) -> AppResult<String> {
        self.exec(Exec::new(script).sudo_if(sudo)).await?.into_stdout()
    }

    /// Try as the login user and transparently retry as root when the
    /// failure is a permission problem.
    pub async fn exec_auto(&self, exec: Exec) -> AppResult<Output> {
        if exec.sudo || self.is_root() {
            return self.exec(exec).await;
        }
        let output = self.exec(exec.clone()).await?;
        if !output.success() && output.permission_denied() {
            return self.exec(exec.sudo()).await;
        }
        Ok(output)
    }

    pub async fn run_auto(&self, script: impl Into<String>) -> AppResult<String> {
        self.exec_auto(Exec::new(script)).await?.into_stdout()
    }

    /// True when the remote command exists in `PATH`.
    pub async fn has_command(&self, name: &'static str) -> AppResult<bool> {
        let out = self.exec(Exec::new(format!("command -v {name} >/dev/null 2>&1"))).await?;
        Ok(out.success())
    }

    pub async fn close(&self) {
        self.shutdown.cancel();
        let streams: Vec<_> = std::mem::take(&mut *self.streams.lock());
        for conn in streams.into_iter().chain(std::iter::once(self.main())) {
            let _ = conn.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        }
    }
}

async fn collect(channel: &mut Channel<Msg>, line: &str, stdin: Option<&[u8]>) -> AppResult<Output> {
    channel.exec(true, line).await?;
    if let Some(data) = stdin {
        channel.data(data).await?;
    }
    // Always close stdin so commands that read it cannot hang.
    channel.eof().await?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut code = None;
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data } => {
                if stdout.len() < MAX_OUTPUT {
                    stdout.extend_from_slice(&data);
                }
            }
            ChannelMsg::ExtendedData { data, ext: 1 } => {
                if stderr.len() < MAX_OUTPUT {
                    stderr.extend_from_slice(&data);
                }
            }
            ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status as i32),
            ChannelMsg::ExitSignal { .. } => code = code.or(Some(-1)),
            _ => {}
        }
    }
    let Some(code) = code else {
        return Err(AppError::new(ErrorCode::ConnectionLost, "channel closed before the command finished"));
    };
    Ok(Output { stdout: String::from_utf8_lossy(&stdout).into_owned(), stderr: String::from_utf8_lossy(&stderr).into_owned(), code })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_line_forces_locale_and_quotes_script() {
        let line = Session::user_line("echo 'hi'");
        assert!(line.starts_with("env LC_ALL=C LANG=C sh -c '"));
        assert!(line.contains("echo '\\''hi'\\''"));
        assert!(line.contains("/usr/sbin"));
    }

    #[test]
    fn output_error_mapping() {
        let ok = Output { stdout: "x".into(), stderr: String::new(), code: 0 };
        assert_eq!(ok.into_stdout().unwrap(), "x");

        let denied = Output { stdout: String::new(), stderr: "cat: /etc/shadow: Permission denied".into(), code: 1 };
        assert_eq!(denied.into_stdout().unwrap_err().code, ErrorCode::PermissionDenied);

        let missing = Output { stdout: String::new(), stderr: "sh: 1: restic: not found".into(), code: 127 };
        assert_eq!(missing.into_stdout().unwrap_err().code, ErrorCode::DependencyMissing);

        let failed = Output { stdout: "partial".into(), stderr: String::new(), code: 2 };
        let err = failed.into_stdout().unwrap_err();
        assert_eq!(err.code, ErrorCode::CommandFailed);
        assert_eq!(err.details.as_deref(), Some("partial"));
    }

    #[test]
    fn tail_respects_char_boundaries() {
        let text = "é".repeat(10);
        let t = tail(&text, 5);
        assert!(t.starts_with('…'));
        assert!(t.len() <= 5 + '…'.len_utf8());
    }
}
