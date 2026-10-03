//! The single error type crossing the IPC boundary.
//!
//! Every failure is an [`ErrorCode`] plus optional free-form details. The code
//! list is exported to TypeScript, where a `Record<ErrorCode, string>` supplies
//! the human message, so an unmapped code is a compile error on the frontend.

use serde::Serialize;
use specta::Type;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    NotConnected,
    ConnectionFailed,
    ConnectionLost,
    AuthFailed,
    HostKeyUnknown,
    HostKeyChanged,
    KeyFileInvalid,
    KeyPassphraseRequired,
    KeyPassphraseWrong,
    Timeout,
    Cancelled,
    SudoPasswordRequired,
    SudoPasswordExpired,
    SudoPasswordWrong,
    SudoLocked,
    SudoUnavailable,
    PermissionDenied,
    NotFound,
    AlreadyExists,
    InvalidInput,
    CommandFailed,
    ParseFailed,
    DependencyMissing,
    Unsupported,
    Io,
    StoreCorrupt,
    Keyring,
    Sftp,
    TransferFailed,
    FileTooLarge,
    BinaryFile,
    Database,
    DbNotConnected,
    Http,
    PangolinNotConfigured,
    PangolinUnauthorized,
    ConfigTestFailed,
    Internal,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AppError {
    pub code: ErrorCode,
    pub details: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, details: impl Into<String>) -> Self {
        let details = details.into();
        let details = details.trim();
        Self {
            code,
            details: (!details.is_empty()).then(|| details.to_string()),
        }
    }

    pub fn code(code: ErrorCode) -> Self {
        Self {
            code,
            details: None,
        }
    }

    pub fn invalid(details: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, details)
    }

    pub fn internal(details: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, details)
    }

    pub fn parse(details: impl Into<String>) -> Self {
        Self::new(ErrorCode::ParseFailed, details)
    }

    pub fn unsupported(details: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unsupported, details)
    }

    pub fn is(&self, code: ErrorCode) -> bool {
        self.code == code
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.details {
            Some(details) => write!(f, "{:?}: {details}", self.code),
            None => write!(f, "{:?}", self.code),
        }
    }
}

impl std::error::Error for AppError {}

impl From<ErrorCode> for AppError {
    fn from(code: ErrorCode) -> Self {
        Self::code(code)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            std::io::ErrorKind::AlreadyExists => ErrorCode::AlreadyExists,
            std::io::ErrorKind::TimedOut => ErrorCode::Timeout,
            _ => ErrorCode::Io,
        };
        Self::new(code, e.to_string())
    }
}

impl From<russh::Error> for AppError {
    fn from(e: russh::Error) -> Self {
        use russh::Error as E;
        let code = match &e {
            E::Disconnect | E::HUP | E::SendError | E::RecvError => ErrorCode::ConnectionLost,
            E::ConnectionTimeout | E::KeepaliveTimeout | E::InactivityTimeout => ErrorCode::Timeout,
            E::NotAuthenticated | E::NoAuthMethod => ErrorCode::AuthFailed,
            E::IO(_) => ErrorCode::ConnectionLost,
            _ => ErrorCode::ConnectionFailed,
        };
        Self::new(code, e.to_string())
    }
}

impl From<russh_sftp::client::error::Error> for AppError {
    fn from(e: russh_sftp::client::error::Error) -> Self {
        use russh_sftp::client::error::Error as E;
        use russh_sftp::protocol::StatusCode;
        match &e {
            E::Status(status) => {
                let code = match status.status_code {
                    StatusCode::NoSuchFile => ErrorCode::NotFound,
                    StatusCode::PermissionDenied => ErrorCode::PermissionDenied,
                    StatusCode::ConnectionLost | StatusCode::NoConnection => {
                        ErrorCode::ConnectionLost
                    }
                    StatusCode::OpUnsupported => ErrorCode::Unsupported,
                    _ => ErrorCode::Sftp,
                };
                Self::new(code, status.error_message.clone())
            }
            E::Timeout => Self::new(ErrorCode::Timeout, e.to_string()),
            _ => Self::new(ErrorCode::Sftp, e.to_string()),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::new(ErrorCode::ParseFailed, e.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        let details = match &e {
            sqlx::Error::Database(db) => db.message().to_string(),
            other => other.to_string(),
        };
        Self::new(ErrorCode::Database, details)
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        let code = if e.is_timeout() {
            ErrorCode::Timeout
        } else {
            ErrorCode::Http
        };
        Self::new(code, e.without_url().to_string())
    }
}

impl From<tokio::time::error::Elapsed> for AppError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        Self::code(ErrorCode::Timeout)
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        Self::internal(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_as_code_and_details() {
        let e = AppError::new(ErrorCode::SudoPasswordRequired, "");
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"code":"SUDO_PASSWORD_REQUIRED","details":null}"#
        );
        let e = AppError::new(ErrorCode::CommandFailed, " boom \n");
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"code":"COMMAND_FAILED","details":"boom"}"#
        );
    }
}
