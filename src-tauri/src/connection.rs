//! Connection lifecycle: connect, heartbeat with auto-reconnect, disconnect,
//! plus the sudo and known-hosts commands that belong to it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::shell::validate;
use crate::ssh::client::{Auth, ConnectFailure, Target};
use crate::ssh::known_hosts::{HostKeyIssue, KnownHost};
use crate::ssh::session::Session;
use crate::state::AppState;
use crate::store::profiles::{AuthType, SECRET_PASSPHRASE, SECRET_PASSWORD};
use crate::sudo::Mode;

const HEARTBEAT: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub profile_id: String,
    pub host: String,
    pub user: String,
    pub home: String,
    pub is_root: bool,
    pub has_sudo: bool,
}

impl SessionInfo {
    fn of(session: &Session) -> Self {
        Self {
            profile_id: session.profile.id.clone(),
            host: session.profile.host.clone(),
            user: session.facts.user.clone(),
            home: session.facts.home.clone(),
            is_root: session.is_root(),
            has_sudo: session.facts.has_sudo,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ConnectOutcome {
    Connected {
        session: SessionInfo,
    },
    /// The server's host key needs a decision before connecting.
    HostKey {
        issue: HostKeyIssue,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LinkState {
    Online,
    Offline,
    Reconnecting,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub profile_id: String,
    pub state: LinkState,
    pub attempt: u32,
    pub error: Option<AppError>,
}

fn target_for(state: &AppState, profile_id: &str) -> AppResult<(crate::store::profiles::Profile, Target)> {
    let profile = state.profiles.get(profile_id)?;
    state.secrets.import_legacy_profile(profile_id);
    let auth = match profile.auth_type {
        AuthType::Password => Auth::Password(
            state
                .secrets
                .get(profile_id, SECRET_PASSWORD)?
                .ok_or_else(|| AppError::new(ErrorCode::AuthFailed, "No password is stored for this profile"))?,
        ),
        AuthType::Key => Auth::Key {
            path: PathBuf::from(profile.key_path.clone().unwrap_or_default()),
            passphrase: state.secrets.get(profile_id, SECRET_PASSPHRASE)?,
        },
    };
    let target = Target { host: profile.host.clone(), port: profile.port, username: profile.username.clone(), auth };
    Ok((profile, target))
}

/// Connect to a profile, replacing any current session.
#[tauri::command]
#[specta::specta]
pub async fn connect(app: AppHandle, state: State<'_, AppState>, profile_id: String) -> AppResult<ConnectOutcome> {
    let (profile, target) = target_for(&state, &profile_id)?;
    state.teardown().await;
    match Session::open(profile, target, state.known_hosts.clone()).await {
        Ok(session) => {
            state.set_session(session.clone());
            spawn_monitor(app, session.clone(), state.reconnect_now.clone());
            Ok(ConnectOutcome::Connected { session: SessionInfo::of(&session) })
        }
        Err(ConnectFailure::HostKey(issue)) => Ok(ConnectOutcome::HostKey { issue: *issue }),
        Err(ConnectFailure::Other(e)) => Err(e),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn disconnect(state: State<'_, AppState>) -> AppResult<()> {
    state.teardown().await;
    Ok(())
}

/// The live session, if any (lets the UI recover after a webview reload).
#[tauri::command]
#[specta::specta]
pub fn session_info(state: State<'_, AppState>) -> Option<SessionInfo> {
    state.session_opt().map(|s| SessionInfo::of(&s))
}

/// Ask the monitor to retry right away instead of waiting out the backoff.
#[tauri::command]
#[specta::specta]
pub fn reconnect_now(state: State<'_, AppState>) {
    state.reconnect_now.notify_waiters();
}

fn spawn_monitor(app: AppHandle, session: Arc<Session>, wake: Arc<tokio::sync::Notify>) {
    let emit = move |app: &AppHandle, session: &Session, state, attempt, error| {
        let _ = ConnectionStatus { profile_id: session.profile.id.clone(), state, attempt, error }.emit(app);
    };
    tauri::async_runtime::spawn(async move {
        let mut strikes = 0;
        loop {
            tokio::select! {
                _ = session.shutdown.cancelled() => return,
                _ = tokio::time::sleep(HEARTBEAT) => {}
            }
            let alive = !session.main_closed() && session.exec_line("true", None, Duration::from_secs(10)).await.is_ok();
            if alive {
                strikes = 0;
                continue;
            }
            // A single slow answer on a busy link is not an outage.
            strikes += 1;
            if strikes < 2 && !session.main_closed() {
                continue;
            }
            strikes = 0;

            emit(&app, &session, LinkState::Offline, 0, None);
            let mut attempt = 0u32;
            loop {
                attempt += 1;
                emit(&app, &session, LinkState::Reconnecting, attempt, None);
                match session.reconnect().await {
                    Ok(()) => {
                        emit(&app, &session, LinkState::Online, attempt, None);
                        break;
                    }
                    Err(e) => emit(&app, &session, LinkState::Offline, attempt, Some(e)),
                }
                let backoff = Duration::from_secs(1u64 << attempt.min(5)).min(MAX_BACKOFF);
                tokio::select! {
                    _ = session.shutdown.cancelled() => return,
                    _ = wake.notified() => {}
                    _ = tokio::time::sleep(backoff) => {}
                }
            }
        }
    });
}

// ---------------------------------------------------------------- sudo

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SudoMode {
    Root,
    Passwordless,
    Password,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SudoStatus {
    pub mode: SudoMode,
    /// Elevated commands can run right now without prompting.
    pub ready: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn sudo_status(state: State<'_, AppState>) -> AppResult<SudoStatus> {
    let session = state.session()?;
    let mode = match session.sudo_mode().await? {
        Mode::Root => SudoMode::Root,
        Mode::Passwordless => SudoMode::Passwordless,
        Mode::Password => SudoMode::Password,
        Mode::Unavailable => SudoMode::Unavailable,
    };
    Ok(SudoStatus { mode, ready: session.sudo_ready().await })
}

/// Verify the sudo password and cache it for the session (15 minutes).
#[tauri::command]
#[specta::specta]
pub async fn sudo_authenticate(state: State<'_, AppState>, password: String) -> AppResult<()> {
    state.session()?.sudo_authenticate(password).await
}

#[tauri::command]
#[specta::specta]
pub fn sudo_forget(state: State<'_, AppState>) {
    if let Some(session) = state.session_opt() {
        session.sudo.invalidate();
    }
}

// ---------------------------------------------------------------- known hosts

#[tauri::command]
#[specta::specta]
pub fn known_hosts_list(state: State<'_, AppState>) -> AppResult<Vec<KnownHost>> {
    state.known_hosts.list()
}

/// Trust a host key shown in the fingerprint prompt. `replace` drops the
/// previously trusted keys for that host (the "key changed" path).
#[tauri::command]
#[specta::specta]
pub fn known_host_trust(state: State<'_, AppState>, host: String, port: u32, key: String, replace: bool) -> AppResult<()> {
    validate::host(&host)?;
    state.known_hosts.trust(&host, validate::port(port)?, &key, replace)
}

#[tauri::command]
#[specta::specta]
pub fn known_host_forget(state: State<'_, AppState>, host: String, port: u32) -> AppResult<()> {
    state.known_hosts.forget(&host, validate::port(port)?)
}
