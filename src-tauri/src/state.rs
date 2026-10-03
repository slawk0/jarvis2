//! Process-wide state shared by all commands.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tokio::sync::Notify;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::jobs::Jobs;
use crate::ssh::known_hosts::KnownHosts;
use crate::ssh::session::Session;
use crate::store::profile_data::ProfileData;
use crate::store::profiles::ProfileStore;
use crate::store::secrets::Secrets;
use crate::store::settings::SettingsStore;
use crate::store::Paths;

pub struct AppState {
    pub paths: Paths,
    pub profiles: ProfileStore,
    pub settings: SettingsStore,
    pub data: ProfileData,
    pub secrets: Secrets,
    pub known_hosts: Arc<KnownHosts>,
    pub jobs: Jobs,
    pub terminals: crate::terminal::Terminals,
    pub transfers: crate::sftp::transfer::Transfers,
    pub databases: crate::db::Databases,
    /// Problems found while loading local data, shown once by the UI.
    pub notices: Mutex<Vec<AppError>>,
    /// Wakes the connection monitor for an immediate reconnect attempt.
    pub reconnect_now: Arc<Notify>,
    session: RwLock<Option<Arc<Session>>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Self {
        let paths = Paths::new(config_dir);
        let mut notices = Vec::new();
        let (profiles, notice) = ProfileStore::load(&paths);
        notices.extend(notice);
        let (settings, notice) = SettingsStore::load(&paths);
        notices.extend(notice);
        Self {
            known_hosts: Arc::new(KnownHosts::new(paths.known_hosts_file())),
            data: ProfileData::new(paths.clone()),
            secrets: Secrets::new(paths.clone()),
            profiles,
            settings,
            paths,
            jobs: Jobs::new(),
            terminals: Default::default(),
            transfers: Default::default(),
            databases: Default::default(),
            notices: Mutex::new(notices),
            reconnect_now: Arc::new(Notify::new()),
            session: RwLock::new(None),
        }
    }

    /// The active server session, or `NOT_CONNECTED`.
    pub fn session(&self) -> AppResult<Arc<Session>> {
        self.session
            .read()
            .clone()
            .ok_or_else(|| AppError::code(ErrorCode::NotConnected))
    }

    pub fn session_opt(&self) -> Option<Arc<Session>> {
        self.session.read().clone()
    }

    pub fn set_session(&self, session: Arc<Session>) {
        *self.session.write() = Some(session);
    }

    /// Tear down everything bound to the current connection.
    pub async fn teardown(&self) {
        let session = self.session.write().take();
        self.jobs.cancel_all();
        self.terminals.close_all();
        self.transfers.cancel_all();
        self.databases.close_all().await;
        if let Some(session) = session {
            session.sudo.invalidate();
            session.close().await;
        }
    }
}
