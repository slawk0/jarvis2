//! End-to-end tests against the disposable server in `dev/test-server`.
//!
//! They are `#[ignore]`d so a plain `cargo test` stays offline. Run them with:
//!
//! ```sh
//! docker compose -f dev/test-server/docker-compose.yml up -d --build
//! JARVIS_TEST_KEY=/path/to/private/key cargo test -- --ignored --test-threads=4
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use crate::error::ErrorCode;
use crate::ssh::client::{Auth, ConnectFailure, Target};
use crate::ssh::known_hosts::KnownHosts;
use crate::ssh::session::{Exec, Session};
use crate::store::profiles::{AuthType, Profile};

pub const HOST: &str = "127.0.0.1";
pub const PORT: u16 = 2222;
pub const ADMIN_PASSWORD: &str = "jarvis-test";

fn profile(user: &str) -> Profile {
    Profile {
        id: "test".into(),
        label: "test".into(),
        host: HOST.into(),
        port: PORT,
        username: user.into(),
        auth_type: AuthType::Password,
        key_path: None,
        is_default: false,
    }
}

fn target(user: &str, auth: Auth) -> Target {
    Target {
        host: HOST.into(),
        port: PORT,
        username: user.into(),
        auth,
    }
}

pub fn test_key() -> PathBuf {
    PathBuf::from(std::env::var("JARVIS_TEST_KEY").expect("set JARVIS_TEST_KEY to the test private key"))
}

fn fresh_known_hosts() -> (tempfile::TempDir, Arc<KnownHosts>) {
    let dir = tempfile::tempdir().unwrap();
    let kh = Arc::new(KnownHosts::new(dir.path().join("known_hosts")));
    (dir, kh)
}

/// Connect, trusting the host key on first contact like the UI prompt would.
pub async fn open(user: &str, auth: Auth) -> (tempfile::TempDir, Arc<Session>) {
    let (dir, kh) = fresh_known_hosts();
    let first = Session::open(profile(user), target(user, auth.clone()), kh.clone()).await;
    let issue = match first {
        Err(ConnectFailure::HostKey(issue)) => issue,
        Err(ConnectFailure::Other(e)) => panic!("connect failed: {e}"),
        Ok(_) => panic!("an unknown host key must not be accepted silently"),
    };
    assert!(issue.known_fingerprints.is_empty());
    assert!(issue.fingerprint.starts_with("SHA256:"));
    kh.trust(HOST, PORT, &issue.key, false).unwrap();
    match Session::open(profile(user), target(user, auth), kh).await {
        Ok(session) => (dir, session),
        Err(f) => panic!("connect after trust failed: {}", crate::error::AppError::from(f)),
    }
}

/// `admin`: password login, sudo needs the password.
pub async fn admin() -> (tempfile::TempDir, Arc<Session>) {
    open("admin", Auth::Password(ADMIN_PASSWORD.into())).await
}

/// `admin` with the sudo password already entered.
pub async fn admin_sudo() -> (tempfile::TempDir, Arc<Session>) {
    let (dir, session) = admin().await;
    session
        .sudo_authenticate(ADMIN_PASSWORD.into())
        .await
        .unwrap();
    (dir, session)
}

/// `keyuser`: key login, passwordless sudo.
pub async fn keyuser() -> (tempfile::TempDir, Arc<Session>) {
    open(
        "keyuser",
        Auth::Key {
            path: test_key(),
            passphrase: None,
        },
    )
    .await
}

#[tokio::test]
#[ignore]
async fn live_password_login_and_facts() {
    let (_d, s) = admin().await;
    assert_eq!(s.facts.user, "admin");
    assert_eq!(s.facts.home, "/home/admin");
    assert!(s.facts.has_sudo);
    assert!(!s.is_root());
    assert_eq!(s.run("echo hello").await.unwrap(), "hello\n");
}

#[tokio::test]
#[ignore]
async fn live_wrong_password_is_auth_failed() {
    let (_dir, kh) = fresh_known_hosts();
    // Trust first so the failure is about authentication.
    let (_d2, s) = admin().await;
    drop(s);
    let issue = match Session::open(profile("admin"), target("admin", Auth::Password("nope".into())), kh.clone()).await {
        Err(ConnectFailure::HostKey(issue)) => issue,
        _ => panic!("expected host key prompt"),
    };
    kh.trust(HOST, PORT, &issue.key, false).unwrap();
    match Session::open(profile("admin"), target("admin", Auth::Password("nope".into())), kh).await {
        Err(ConnectFailure::Other(e)) => assert_eq!(e.code, ErrorCode::AuthFailed),
        _ => panic!("expected AUTH_FAILED"),
    }
}

#[tokio::test]
#[ignore]
async fn live_changed_host_key_is_reported() {
    let (_dir, kh) = fresh_known_hosts();
    // Pretend we trusted a different key for this host earlier.
    kh.trust(
        HOST,
        PORT,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti",
        false,
    )
    .unwrap();
    match Session::open(profile("admin"), target("admin", Auth::Password(ADMIN_PASSWORD.into())), kh).await {
        Err(ConnectFailure::HostKey(issue)) => assert_eq!(issue.known_fingerprints.len(), 1),
        _ => panic!("a changed host key must block the connection"),
    }
}

#[tokio::test]
#[ignore]
async fn live_key_login_with_and_without_passphrase() {
    let (_d, s) = keyuser().await;
    assert_eq!(s.facts.user, "keyuser");

    let encrypted = PathBuf::from(format!("{}_pp", test_key().display()));
    let (_d, s) = open(
        "keyuser",
        Auth::Key {
            path: encrypted.clone(),
            passphrase: Some("secret-pass".into()),
        },
    )
    .await;
    assert_eq!(s.run("whoami").await.unwrap().trim(), "keyuser");

    for (passphrase, code) in [
        (None, ErrorCode::KeyPassphraseRequired),
        (Some("wrong".to_string()), ErrorCode::KeyPassphraseWrong),
    ] {
        let (_dir, kh) = fresh_known_hosts();
        let auth = Auth::Key {
            path: encrypted.clone(),
            passphrase,
        };
        let issue = match Session::open(profile("keyuser"), target("keyuser", auth.clone()), kh.clone()).await {
            Err(ConnectFailure::HostKey(issue)) => issue,
            _ => panic!("expected host key prompt"),
        };
        kh.trust(HOST, PORT, &issue.key, false).unwrap();
        match Session::open(profile("keyuser"), target("keyuser", auth), kh).await {
            Err(ConnectFailure::Other(e)) => assert_eq!(e.code, code),
            _ => panic!("expected {code:?}"),
        }
    }
}

#[tokio::test]
#[ignore]
async fn live_sudo_prompts_then_runs_as_root() {
    let (_d, s) = admin().await;
    // Without a password the service asks for one instead of failing oddly.
    let err = s.run_sudo("id -u").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::SudoPasswordRequired);

    let err = s.sudo_authenticate("wrong".into()).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::SudoPasswordWrong);
    assert_eq!(err.details.as_deref(), Some("4"));

    s.sudo_authenticate(ADMIN_PASSWORD.into()).await.unwrap();
    assert_eq!(s.run_sudo("id -u").await.unwrap().trim(), "0");

    // stdin still reaches the command after the password line.
    let out = s.exec(Exec::new("cat").sudo().stdin("payload")).await.unwrap();
    assert_eq!(out.stdout, "payload");
    assert!(!out.stderr.contains("jarvis"), "prompt marker leaked: {:?}", out.stderr);

    // The transparent fallback only elevates on a permission problem.
    assert!(s.run("cat /etc/shadow").await.is_err());
    assert!(s.run_auto("cat /etc/shadow").await.unwrap().contains("root:"));
}

#[tokio::test]
#[ignore]
async fn live_sudo_lockout_after_five_failures() {
    let (_d, s) = admin().await;
    for _ in 0..4 {
        let e = s.sudo_authenticate("bad".into()).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::SudoPasswordWrong);
    }
    let e = s.sudo_authenticate("bad".into()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::SudoLocked);
    // Even the right password is refused while locked.
    let e = s.sudo_authenticate(ADMIN_PASSWORD.into()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::SudoLocked);
}

#[tokio::test]
#[ignore]
async fn live_passwordless_sudo_needs_no_prompt() {
    let (_d, s) = keyuser().await;
    assert_eq!(s.run_sudo("id -u").await.unwrap().trim(), "0");
    let out = s.exec(Exec::new("cat").sudo().stdin("x")).await.unwrap();
    assert_eq!(out.stdout, "x");
}

#[tokio::test]
#[ignore]
async fn live_exec_edge_cases() {
    let (_d, s) = admin().await;
    // Forced C locale although the server default is Polish.
    let out = s.exec(Exec::new("ls /definitely-missing")).await.unwrap();
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("No such file or directory"), "{}", out.stderr);
    // Timeouts close the channel instead of hanging.
    let err = s.exec(Exec::new("sleep 30").secs(1)).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Timeout);
    // sbin is on PATH for regular users.
    assert!(s.run("command -v ufw").await.is_ok());
    // UTF-8 round trip and quoting.
    assert_eq!(s.run("printf '%s' 'zażółć $HOME `x`'").await.unwrap(), "zażółć $HOME `x`");
    // Many concurrent commands share the connection without tripping MaxSessions.
    let runs = (0..24).map(|i| {
        let s = s.clone();
        async move { s.run(format!("echo {i}")).await }
    });
    let results = futures::future::join_all(runs).await;
    for (i, r) in results.into_iter().enumerate() {
        assert_eq!(r.unwrap().trim(), i.to_string());
    }
}
