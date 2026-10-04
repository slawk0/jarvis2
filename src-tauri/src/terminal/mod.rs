//! Interactive PTY terminals. Each session gets its own SSH connection so a
//! busy terminal never competes with commands, and so it can simply be
//! dropped when the session ends.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use russh::ChannelMsg;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::shell::{q, validate, Cmd};
use crate::ssh::client::Auth;
use crate::ssh::session::Session;
use crate::state::AppState;
use crate::sudo::Plan;
use crate::text::{TokenFilter, Utf8Chunker};

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalTarget {
    /// The login shell of the connected user.
    Host,
    /// `docker exec -it <container> <shell>`.
    Container { container: String, shell: String },
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct TerminalData {
    pub id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExit {
    pub id: String,
    pub exit_code: Option<i32>,
    /// Set when the terminal ended because of an error (e.g. connection lost).
    pub error: Option<AppError>,
}

enum Input {
    Data(Vec<u8>),
    Resize(u32, u32),
    Close,
}

struct Handle {
    tx: mpsc::UnboundedSender<Input>,
    /// PID of the remote shell, for "current directory" lookups.
    pid: Arc<Mutex<Option<u32>>>,
}

#[derive(Default)]
pub struct Terminals {
    open: Arc<Mutex<HashMap<String, Handle>>>,
}

impl Terminals {
    pub fn close_all(&self) {
        for (_, handle) in self.open.lock().drain() {
            let _ = handle.tx.send(Input::Close);
        }
    }

    /// PIDs of the remote shells of the open host terminals.
    pub fn shell_pids(&self) -> Vec<u32> {
        self.open.lock().values().filter_map(|h| *h.pid.lock()).collect()
    }

    fn send(&self, id: &str, input: Input) -> AppResult<()> {
        let open = self.open.lock();
        let handle = open.get(id).ok_or_else(|| AppError::new(ErrorCode::NotFound, "terminal"))?;
        handle.tx.send(input).map_err(|_| AppError::new(ErrorCode::NotFound, "terminal"))
    }
}

const SHELLS: [&str; 5] = ["bash", "sh", "ash", "zsh", "fish"];

/// What to run in the PTY, and the sudo password to feed once the remote
/// side signals it is ready to read it without echo.
async fn launch_plan(session: &Session, target: &TerminalTarget) -> AppResult<(String, Option<String>)> {
    match target {
        // Report the shell's PID, then become the user's login shell.
        TerminalTarget::Host => Ok(("printf '[[jarvis:pid:%s]]' \"$$\"; exec \"${SHELL:-/bin/sh}\" -l".to_string(), None)),
        TerminalTarget::Container { container, shell } => {
            validate::name("container", container)?;
            validate::one_of("shell", shell, &SHELLS)?;
            let exec = Cmd::new("docker").lit("exec -it").arg(container).arg(shell).build();
            if !crate::docker::needs_sudo(session).await? {
                return Ok((format!("exec {exec}"), None));
            }
            match session.sudo_plan().await? {
                Plan::Direct => Ok((format!("exec {exec}"), None)),
                Plan::NoPassword => Ok((format!("exec sudo -n {exec}"), None)),
                Plan::WithPassword(password) => Ok((
                    // Echo is off before the password is requested, so it never shows
                    // in the terminal; `sudo -v` then primes this TTY's credentials.
                    format!(
                        "stty -echo; printf '[[jarvis:ready]]'; IFS= read -r p; stty echo; \
                         if printf '%s\\n' \"$p\" | sudo -S -p '' -v 2>/dev/null; then unset p; exec sudo {exec}; \
                         else echo 'sudo authentication failed'; fi"
                    ),
                    Some(password),
                )),
            }
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn terminal_open(app: AppHandle, state: State<'_, AppState>, target: TerminalTarget, cols: u32, rows: u32) -> AppResult<String> {
    let session = state.session()?;
    let (command, mut password) = launch_plan(&session, &target).await?;
    let conn = session.dedicated().await?;
    let mut channel = conn.handle().channel_open_session().await?;
    channel.request_pty(true, "xterm-256color", cols.max(2), rows.max(2), 0, 0, &[]).await?;
    let _ = channel.set_env(false, "LANG", "C.UTF-8").await;
    channel.exec(true, format!("sh -c {}", q(&command))).await?;

    let id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let pid = Arc::new(Mutex::new(None));
    state.terminals.open.lock().insert(id.clone(), Handle { tx, pid: pid.clone() });

    let open = state.terminals.open.clone();
    let terminal_id = id.clone();
    let shutdown = session.shutdown.clone();
    tauri::async_runtime::spawn(async move {
        // Keeps the dedicated connection alive for the terminal's lifetime.
        let _conn = conn;
        let mut decoder = Utf8Chunker::new();
        let mut filter = TokenFilter::new();
        let mut exit_code = None;
        let mut error = None;

        enum Step {
            Remote(Option<ChannelMsg>),
            Local(Option<Input>),
            Shutdown,
        }

        loop {
            let step = tokio::select! {
                msg = channel.wait() => Step::Remote(msg),
                input = rx.recv() => Step::Local(input),
                _ = shutdown.cancelled() => Step::Shutdown,
            };
            match step {
                Step::Remote(Some(ChannelMsg::Data { data })) | Step::Remote(Some(ChannelMsg::ExtendedData { data, .. })) => {
                    let (text, tokens) = filter.push(&decoder.push(&data));
                    for token in tokens {
                        if let Some(p) = token.strip_prefix("pid:") {
                            *pid.lock() = p.parse().ok();
                        } else if token == "ready" {
                            if let Some(pw) = password.take() {
                                let _ = channel.data(format!("{pw}\n").as_bytes()).await;
                            }
                        }
                    }
                    if !text.is_empty() {
                        let _ = TerminalData { id: terminal_id.clone(), data: text }.emit(&app);
                    }
                }
                Step::Remote(Some(ChannelMsg::ExitStatus { exit_status })) => {
                    exit_code = Some(exit_status as i32);
                }
                Step::Remote(Some(_)) => {}
                Step::Remote(None) => {
                    if exit_code.is_none() {
                        error = Some(AppError::code(ErrorCode::ConnectionLost));
                    }
                    break;
                }
                Step::Local(Some(Input::Data(bytes))) => {
                    if channel.data(&bytes[..]).await.is_err() {
                        error = Some(AppError::code(ErrorCode::ConnectionLost));
                        break;
                    }
                }
                Step::Local(Some(Input::Resize(cols, rows))) => {
                    let _ = channel.window_change(cols, rows, 0, 0).await;
                }
                Step::Local(Some(Input::Close)) | Step::Local(None) | Step::Shutdown => {
                    let _ = channel.close().await;
                    open.lock().remove(&terminal_id);
                    return;
                }
            }
        }

        let rest = format!("{}{}", decoder.finish(), filter.finish());
        if !rest.is_empty() {
            let _ = TerminalData { id: terminal_id.clone(), data: rest }.emit(&app);
        }
        open.lock().remove(&terminal_id);
        let _ = TerminalExit { id: terminal_id, exit_code, error }.emit(&app);
    });

    Ok(id)
}

#[tauri::command]
#[specta::specta]
pub fn terminal_write(state: State<'_, AppState>, id: String, data: String) -> AppResult<()> {
    state.terminals.send(&id, Input::Data(data.into_bytes()))
}

#[tauri::command]
#[specta::specta]
pub fn terminal_resize(state: State<'_, AppState>, id: String, cols: u32, rows: u32) -> AppResult<()> {
    state.terminals.send(&id, Input::Resize(cols.max(2), rows.max(2)))
}

#[tauri::command]
#[specta::specta]
pub fn terminal_close(state: State<'_, AppState>, id: String) {
    let _ = state.terminals.send(&id, Input::Close);
}

/// Current directory of a host terminal's shell, if it can be determined.
#[tauri::command]
#[specta::specta]
pub async fn terminal_cwd(state: State<'_, AppState>, id: String) -> AppResult<Option<String>> {
    let pid = state.terminals.open.lock().get(&id).and_then(|h| *h.pid.lock());
    let Some(pid) = pid else {
        return Ok(None);
    };
    let session = state.session()?;
    let out = session.run_auto(format!("readlink /proc/{pid}/cwd")).await.unwrap_or_default();
    let cwd = out.trim();
    Ok((!cwd.is_empty()).then(|| cwd.to_string()))
}

/// Arguments for the system `ssh` client. Never contains a password.
fn ssh_args(session: &Session) -> Vec<String> {
    let target = session.target();
    let mut args = Vec::new();
    if target.port != 22 {
        args.push("-p".to_string());
        args.push(target.port.to_string());
    }
    if let Auth::Key { path, .. } = &target.auth {
        args.push("-i".to_string());
        args.push(path.to_string_lossy().into_owned());
    }
    args.push(format!("{}@{}", target.username, target.host));
    args
}

/// Open the operating system's terminal with an `ssh` session to this server.
#[tauri::command]
#[specta::specta]
pub fn terminal_open_external(state: State<'_, AppState>) -> AppResult<()> {
    use std::process::Command;
    let session = state.session()?;
    let args = ssh_args(&session);

    #[cfg(target_os = "windows")]
    let attempts: Vec<Command> = {
        let mut wt = Command::new("wt.exe");
        wt.arg("ssh").args(&args);
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/c", "start", "", "cmd.exe", "/k", "ssh"]).args(&args);
        vec![wt, cmd]
    };

    #[cfg(target_os = "macos")]
    let attempts: Vec<Command> = {
        let line = std::iter::once("ssh".to_string())
            .chain(args.iter().map(|a| q(a)))
            .collect::<Vec<_>>()
            .join(" ")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let mut osa = Command::new("osascript");
        osa.args([
            "-e",
            &format!("tell application \"Terminal\" to do script \"{line}\""),
            "-e",
            "tell application \"Terminal\" to activate",
        ]);
        vec![osa]
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let attempts: Vec<Command> = {
        // (binary, flag that introduces the command to run)
        const EMULATORS: [(&str, &str); 9] = [
            ("x-terminal-emulator", "-e"),
            ("gnome-terminal", "--"),
            ("konsole", "-e"),
            ("xfce4-terminal", "-x"),
            ("kitty", "--"),
            ("alacritty", "-e"),
            ("wezterm", "-e"),
            ("foot", "--"),
            ("xterm", "-e"),
        ];
        EMULATORS
            .iter()
            .map(|(bin, flag)| {
                let mut c = Command::new(bin);
                c.arg(flag).arg("ssh").args(&args);
                c
            })
            .collect()
    };

    for mut command in attempts {
        if command.spawn().is_ok() {
            return Ok(());
        }
    }
    Err(AppError::unsupported("No terminal emulator could be started"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_shells_and_bad_container_names() {
        assert!(validate::one_of("shell", "bash", &SHELLS).is_ok());
        assert!(validate::one_of("shell", "bash; reboot", &SHELLS).is_err());
        assert!(validate::name("container", "web_1").is_ok());
        assert!(validate::name("container", "$(id)").is_err());
    }
}
