//! Opening and authenticating one SSH connection.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use russh::client;
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate};

use super::known_hosts::{HostKeyIssue, KnownHosts, Verdict};
use crate::error::{AppError, AppResult, ErrorCode};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const AUTH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub enum Auth {
    Password(String),
    Key { path: PathBuf, passphrase: Option<String> },
}

/// Everything needed to (re)open a connection to the active server.
#[derive(Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
}

pub enum ConnectFailure {
    /// The host key is unknown or changed; the user has to decide.
    HostKey(Box<HostKeyIssue>),
    Other(AppError),
}

impl From<ConnectFailure> for AppError {
    fn from(f: ConnectFailure) -> Self {
        match f {
            ConnectFailure::HostKey(issue) => {
                let code = if issue.known_fingerprints.is_empty() { ErrorCode::HostKeyUnknown } else { ErrorCode::HostKeyChanged };
                AppError::new(code, issue.fingerprint)
            }
            ConnectFailure::Other(e) => e,
        }
    }
}

impl From<AppError> for ConnectFailure {
    fn from(e: AppError) -> Self {
        ConnectFailure::Other(e)
    }
}

pub struct Handler {
    host: String,
    port: u16,
    known_hosts: Arc<KnownHosts>,
    issue: Arc<Mutex<Option<HostKeyIssue>>>,
}

impl client::Handler for Handler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_key else {
            // Host certificates would need a trusted CA list, which Jarvis does not keep.
            return Ok(false);
        };
        let known = match self.known_hosts.check(&self.host, self.port, key) {
            Ok(Verdict::Trusted) => return Ok(true),
            Ok(Verdict::Unknown) => Vec::new(),
            Ok(Verdict::Changed(known)) => known,
            Err(_) => return Ok(false),
        };
        *self.issue.lock() = Some(self.known_hosts.issue(&self.host, self.port, key, known));
        Ok(false)
    }
}

pub type Handle = client::Handle<Handler>;

fn config() -> Arc<client::Config> {
    Arc::new(client::Config {
        inactivity_timeout: None,
        keepalive_interval: Some(Duration::from_secs(10)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    })
}

/// Connect, verify the host key against the shared store and authenticate.
pub async fn connect(target: &Target, known_hosts: Arc<KnownHosts>) -> Result<Handle, ConnectFailure> {
    let issue = Arc::new(Mutex::new(None));
    let handler = Handler { host: target.host.clone(), port: target.port, known_hosts, issue: issue.clone() };
    let addr = (target.host.as_str(), target.port);
    let connecting = client::connect(config(), addr, handler);
    let mut handle = match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
        Err(_) => return Err(AppError::new(ErrorCode::Timeout, format!("No answer from {}:{}", target.host, target.port)).into()),
        Ok(Err(e)) => {
            if let Some(issue) = issue.lock().take() {
                return Err(ConnectFailure::HostKey(Box::new(issue)));
            }
            return Err(AppError::new(ErrorCode::ConnectionFailed, e.to_string()).into());
        }
        Ok(Ok(handle)) => handle,
    };

    let authed = tokio::time::timeout(AUTH_TIMEOUT, authenticate(&mut handle, target))
        .await
        .map_err(|_| AppError::new(ErrorCode::Timeout, "Authentication timed out"))??;
    if !authed {
        return Err(AppError::new(ErrorCode::AuthFailed, format!("{}@{}", target.username, target.host)).into());
    }
    Ok(handle)
}

async fn authenticate(handle: &mut Handle, target: &Target) -> AppResult<bool> {
    match &target.auth {
        Auth::Password(password) => Ok(handle.authenticate_password(target.username.clone(), password.clone()).await?.success()),
        Auth::Key { path, passphrase } => {
            let key = load_key(path, passphrase.as_deref())?;
            // Old servers only accept ssh-rsa (SHA-1); newer ones want rsa-sha2-*.
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            Ok(handle.authenticate_publickey(target.username.clone(), PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await?.success())
        }
    }
}

fn load_key(path: &std::path::Path, passphrase: Option<&str>) -> AppResult<russh::keys::PrivateKey> {
    use russh::keys::Error as KeyError;
    let passphrase = passphrase.filter(|p| !p.is_empty());
    match russh::keys::load_secret_key(path, passphrase) {
        Ok(key) => Ok(key),
        Err(KeyError::KeyIsEncrypted) => Err(AppError::code(ErrorCode::KeyPassphraseRequired)),
        Err(KeyError::IO(e)) => Err(AppError::new(ErrorCode::KeyFileInvalid, format!("{}: {e}", path.display()))),
        Err(e) if passphrase.is_some() => Err(AppError::new(ErrorCode::KeyPassphraseWrong, e.to_string())),
        Err(e) => {
            // ssh-key reports an encrypted key without a passphrase as a crypto error.
            let text = e.to_string();
            let code =
                if text.to_ascii_lowercase().contains("encrypt") { ErrorCode::KeyPassphraseRequired } else { ErrorCode::KeyFileInvalid };
            Err(AppError::new(code, text))
        }
    }
}
