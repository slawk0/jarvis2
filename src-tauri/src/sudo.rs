//! The one sudo implementation every feature goes through.
//!
//! A verified password is cached in memory for 15 minutes and fed to
//! `sudo -S` over the channel's stdin; it never appears on a command line.
//! Five wrong passwords lock further attempts for 60 seconds.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::shell::q;
use crate::ssh::session::{Session, SUDO_PROMPT};
use crate::text::strip_tokens;

pub const CACHE_TTL: Duration = Duration::from_secs(15 * 60);
pub const MAX_FAILURES: u32 = 5;
pub const LOCKOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Logged in as root: nothing to elevate.
    Root,
    /// `sudo -n` works (NOPASSWD).
    Passwordless,
    /// sudo needs the user's password.
    Password,
    /// No sudo binary on the server.
    Unavailable,
}

/// How to run the next elevated command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Direct,
    NoPassword,
    WithPassword(String),
}

#[derive(Default)]
struct Inner {
    mode: Option<Mode>,
    password: Option<(String, Instant)>,
    /// Set once a cached password aged out, to tell "expired" from "never given".
    had_password: bool,
    failures: u32,
    locked_until: Option<Instant>,
}

#[derive(Default)]
pub struct Sudo {
    inner: Mutex<Inner>,
}

impl Sudo {
    pub fn new() -> Self {
        Self::default()
    }

    fn mode(&self) -> Option<Mode> {
        self.inner.lock().mode
    }

    fn set_mode(&self, mode: Mode) {
        self.inner.lock().mode = Some(mode);
    }

    /// Forget the cached password (it was rejected, or the session ended).
    pub fn invalidate(&self) {
        let mut inner = self.inner.lock();
        inner.password = None;
        inner.had_password = false;
    }

    fn plan(&self, mode: Mode, now: Instant) -> AppResult<Plan> {
        match mode {
            Mode::Root => Ok(Plan::Direct),
            Mode::Passwordless => Ok(Plan::NoPassword),
            Mode::Unavailable => Err(AppError::code(ErrorCode::SudoUnavailable)),
            Mode::Password => {
                let mut inner = self.inner.lock();
                match &inner.password {
                    Some((password, verified)) if now.duration_since(*verified) < CACHE_TTL => Ok(Plan::WithPassword(password.clone())),
                    Some(_) => {
                        inner.password = None;
                        inner.had_password = true;
                        Err(AppError::code(ErrorCode::SudoPasswordExpired))
                    }
                    None if inner.had_password => Err(AppError::code(ErrorCode::SudoPasswordExpired)),
                    None => Err(AppError::code(ErrorCode::SudoPasswordRequired)),
                }
            }
        }
    }

    /// Seconds left on the lockout, if any.
    fn locked_for(&self, now: Instant) -> Option<u64> {
        let mut inner = self.inner.lock();
        match inner.locked_until {
            Some(until) if until > now => Some((until - now).as_secs().max(1)),
            Some(_) => {
                inner.locked_until = None;
                None
            }
            None => None,
        }
    }

    fn record_success(&self, password: String, now: Instant) {
        let mut inner = self.inner.lock();
        inner.password = Some((password, now));
        inner.had_password = false;
        inner.failures = 0;
        inner.locked_until = None;
    }

    /// Returns the error to report for a rejected password.
    fn record_failure(&self, now: Instant) -> AppError {
        let mut inner = self.inner.lock();
        inner.failures += 1;
        if inner.failures >= MAX_FAILURES {
            inner.failures = 0;
            inner.locked_until = Some(now + LOCKOUT);
            AppError::new(ErrorCode::SudoLocked, LOCKOUT.as_secs().to_string())
        } else {
            AppError::new(ErrorCode::SudoPasswordWrong, (MAX_FAILURES - inner.failures).to_string())
        }
    }
}

impl Session {
    /// Detect once how elevation works on this server.
    pub async fn sudo_mode(&self) -> AppResult<Mode> {
        if let Some(mode) = self.sudo.mode() {
            return Ok(mode);
        }
        let mode = if self.is_root() {
            Mode::Root
        } else if !self.facts.has_sudo {
            Mode::Unavailable
        } else {
            // `-k` ignores cached credentials, so this only succeeds with NOPASSWD.
            let probe = self.exec_line("sudo -k -n true", None, Duration::from_secs(15)).await?;
            if probe.success() {
                Mode::Passwordless
            } else {
                Mode::Password
            }
        };
        self.sudo.set_mode(mode);
        Ok(mode)
    }

    pub async fn sudo_plan(&self) -> AppResult<Plan> {
        let mode = self.sudo_mode().await?;
        self.sudo.plan(mode, Instant::now())
    }

    /// True when elevated commands can run right now without asking the user.
    pub async fn sudo_ready(&self) -> bool {
        self.sudo_plan().await.is_ok()
    }

    /// Verify a password with sudo itself and cache it on success.
    pub async fn sudo_authenticate(&self, password: String) -> AppResult<()> {
        match self.sudo_mode().await? {
            Mode::Root | Mode::Passwordless => return Ok(()),
            Mode::Unavailable => return Err(AppError::code(ErrorCode::SudoUnavailable)),
            Mode::Password => {}
        }
        if let Some(secs) = self.sudo.locked_for(Instant::now()) {
            return Err(AppError::new(ErrorCode::SudoLocked, secs.to_string()));
        }
        if password.is_empty() || password.contains('\n') {
            return Err(self.sudo.record_failure(Instant::now()));
        }
        let line = format!("env LC_ALL=C sudo -k -S -p {} true", q(SUDO_PROMPT));
        let stdin = format!("{password}\n");
        let out = self.exec_line(&line, Some(stdin.as_bytes()), Duration::from_secs(20)).await?;
        let (_, tokens) = strip_tokens(&out.stderr);
        let prompts = tokens.iter().filter(|t| *t == "sudo").count();
        if out.success() && prompts <= 1 {
            self.sudo.record_success(password, Instant::now());
            Ok(())
        } else {
            Err(self.sudo.record_failure(Instant::now()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_by_mode() {
        let sudo = Sudo::new();
        let now = Instant::now();
        assert_eq!(sudo.plan(Mode::Root, now).unwrap(), Plan::Direct);
        assert_eq!(sudo.plan(Mode::Passwordless, now).unwrap(), Plan::NoPassword);
        assert_eq!(sudo.plan(Mode::Unavailable, now).unwrap_err().code, ErrorCode::SudoUnavailable);
        assert_eq!(sudo.plan(Mode::Password, now).unwrap_err().code, ErrorCode::SudoPasswordRequired);
    }

    #[test]
    fn password_is_cached_for_fifteen_minutes() {
        let sudo = Sudo::new();
        let t0 = Instant::now();
        sudo.record_success("pw".into(), t0);
        assert_eq!(sudo.plan(Mode::Password, t0 + Duration::from_secs(14 * 60)).unwrap(), Plan::WithPassword("pw".into()));
        let late = t0 + CACHE_TTL + Duration::from_secs(1);
        assert_eq!(sudo.plan(Mode::Password, late).unwrap_err().code, ErrorCode::SudoPasswordExpired);
        // Still reported as expired until the user authenticates again.
        assert_eq!(sudo.plan(Mode::Password, late).unwrap_err().code, ErrorCode::SudoPasswordExpired);
        sudo.record_success("pw2".into(), late);
        assert_eq!(sudo.plan(Mode::Password, late).unwrap(), Plan::WithPassword("pw2".into()));
    }

    #[test]
    fn five_failures_lock_for_sixty_seconds() {
        let sudo = Sudo::new();
        let t0 = Instant::now();
        for left in (1..MAX_FAILURES).rev() {
            let err = sudo.record_failure(t0);
            assert_eq!(err.code, ErrorCode::SudoPasswordWrong);
            assert_eq!(err.details.as_deref(), Some(left.to_string().as_str()));
        }
        assert_eq!(sudo.locked_for(t0), None);
        let err = sudo.record_failure(t0);
        assert_eq!(err.code, ErrorCode::SudoLocked);
        assert_eq!(sudo.locked_for(t0 + Duration::from_secs(10)), Some(50));
        assert_eq!(sudo.locked_for(t0 + LOCKOUT + Duration::from_secs(1)), None);
        // Counter restarts after the lockout.
        assert_eq!(sudo.record_failure(t0 + LOCKOUT + Duration::from_secs(2)).code, ErrorCode::SudoPasswordWrong);
    }

    #[test]
    fn success_resets_failures_and_invalidate_clears() {
        let sudo = Sudo::new();
        let t0 = Instant::now();
        for _ in 0..3 {
            sudo.record_failure(t0);
        }
        sudo.record_success("pw".into(), t0);
        assert_eq!(sudo.record_failure(t0).details.as_deref(), Some((MAX_FAILURES - 1).to_string().as_str()));
        sudo.invalidate();
        assert_eq!(sudo.plan(Mode::Password, t0).unwrap_err().code, ErrorCode::SudoPasswordRequired);
    }
}
