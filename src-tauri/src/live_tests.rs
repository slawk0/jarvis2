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
    Target { host: HOST.into(), port: PORT, username: user.into(), auth }
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
    session.sudo_authenticate(ADMIN_PASSWORD.into()).await.unwrap();
    (dir, session)
}

/// `keyuser`: key login, passwordless sudo.
pub async fn keyuser() -> (tempfile::TempDir, Arc<Session>) {
    open("keyuser", Auth::Key { path: test_key(), passphrase: None }).await
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
    kh.trust(HOST, PORT, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti", false).unwrap();
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
    let (_d, s) = open("keyuser", Auth::Key { path: encrypted.clone(), passphrase: Some("secret-pass".into()) }).await;
    assert_eq!(s.run("whoami").await.unwrap().trim(), "keyuser");

    for (passphrase, code) in [(None, ErrorCode::KeyPassphraseRequired), (Some("wrong".to_string()), ErrorCode::KeyPassphraseWrong)] {
        let (_dir, kh) = fresh_known_hosts();
        let auth = Auth::Key { path: encrypted.clone(), passphrase };
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

#[tokio::test]
#[ignore]
async fn live_stats() {
    let (_d, s) = admin().await;
    let basic = crate::stats::basic(&s).await.unwrap();
    assert!(basic.cpu_total > basic.cpu_idle);
    assert!(basic.mem_total > basic.mem_available);
    assert!(basic.cpu_count >= 1);
    assert!(basic.root_total > 0);
    assert!(!basic.net_interface.is_empty());
    let ext = crate::stats::extended(&s).await.unwrap();
    assert!(ext.system.os.contains("Ubuntu"), "{}", ext.system.os);
    assert_eq!(ext.system.hostname, "jarvis-test");
    assert!(!ext.top_processes.is_empty());
    let procs = crate::stats::processes(&s).await.unwrap();
    assert!(procs.iter().any(|p| p.pid == 1));
    assert!(procs.iter().any(|p| p.command.contains("sshd")));
}

#[tokio::test]
#[ignore]
async fn live_files_roundtrip_and_sudo_fallback() {
    use crate::files;
    let (_d, s) = admin_sudo().await;
    let dir = format!("/home/admin/jarvis-live-{}", uuid::Uuid::new_v4().simple());
    s.run(format!(
        "mkdir -p '{dir}/sub dir' && printf 'hello' > '{dir}/a file.txt' && ln -s 'sub dir' '{dir}/link' && touch '{dir}/.hidden'"
    ))
    .await
    .unwrap();

    let listing = files::list(&s, &dir).await.unwrap();
    assert_eq!(listing.path, dir);
    assert!(!listing.elevated);
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    for expected in ["sub dir", "a file.txt", "link", ".hidden"] {
        assert!(names.contains(&expected), "{names:?}");
    }
    let link = listing.entries.iter().find(|e| e.name == "link").unwrap();
    assert_eq!(link.kind, files::FileKind::Symlink);
    assert!(link.is_dir_like);
    let file = listing.entries.iter().find(|e| e.name == "a file.txt").unwrap();
    assert_eq!((file.size, file.owner.as_str()), (5, "admin"));

    // Text round trip, including characters the shell would mangle.
    let path = format!("{dir}/a file.txt");
    let text = "zażółć\n$HOME `id` 'q' \"dq\"\n";
    assert!(!files::write_text(&s, &path, text).await.unwrap());
    let read = files::read_text(&s, &path, files::MAX_EDIT_BYTES).await.unwrap();
    assert_eq!(read.content, text);
    assert!(!read.elevated);

    // Root-only locations are listed, read and written through sudo.
    let root_listing = files::list(&s, "/root").await.unwrap();
    assert!(root_listing.elevated);
    let shadow = files::read_text(&s, "/etc/shadow", files::MAX_EDIT_BYTES).await.unwrap();
    assert!(shadow.elevated && shadow.content.contains("root:"));
    let root_file = format!("/root/jarvis-live-{}", uuid::Uuid::new_v4().simple());
    s.run_sudo(format!("touch {root_file}")).await.unwrap();
    assert!(files::write_text(&s, &root_file, "x").await.unwrap());
    s.run_sudo(format!("rm -f {root_file}")).await.unwrap();

    // Guards.
    let e = files::read_text(&s, "/usr/bin/sudo", files::MAX_EDIT_BYTES).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::BinaryFile);
    let e = files::read_text(&s, &path, 3).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::FileTooLarge);
    let e = files::list(&s, "/does/not/exist").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);

    s.run(format!("rm -rf '{dir}'")).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn live_system_modules() {
    let (_d, s) = admin_sudo().await;

    let services = crate::systemd::list_services(&s).await.unwrap();
    let ssh = services.iter().find(|u| u.name == "ssh.service").expect("ssh.service listed");
    assert_eq!((ssh.active.as_str(), ssh.enabled.as_str()), ("active", "enabled"));
    assert!(services.len() > 10);

    let timers = crate::systemd::list_timers(&s).await.unwrap();
    assert!(timers.iter().any(|t| t.unit.ends_with(".timer") && !t.activates.is_empty()), "{timers:?}");
    assert!(timers.iter().any(|t| t.next.is_some()), "no timer has a next run: {timers:?}");

    // Crontab round trip, restoring whatever was there.
    let original = crate::cron::read(&s, false).await.unwrap();
    let with = crate::cron::add_job(&original, "*/7 * * * *", "echo 'jarvis live test' >/dev/null").unwrap();
    crate::cron::write(&s, &with, false).await.unwrap();
    let read_back = crate::cron::read(&s, false).await.unwrap();
    assert_eq!(read_back, with);
    let job = crate::cron::parse(&read_back).into_iter().find(|j| j.command.contains("jarvis live test")).unwrap();
    assert_eq!(job.schedule, "*/7 * * * *");
    crate::cron::write(&s, &original, false).await.unwrap();
    assert!(crate::cron::read(&s, true).await.is_ok());

    let status = crate::packages::status(&s).await.unwrap();
    assert_eq!(status.package_manager, Some(crate::deps::PackageManager::Apt));

    let accounts = crate::users::list(&s).await.unwrap();
    assert_eq!(accounts.current_user, "admin");
    let admin = accounts.users.iter().find(|u| u.name == "admin").unwrap();
    assert!(admin.groups.contains(&"sudo".to_string()));
    assert_eq!(admin.locked, Some(false));
    let keyuser = accounts.users.iter().find(|u| u.name == "keyuser").unwrap();
    assert_eq!(keyuser.locked, Some(true));
    assert!(accounts.shells.contains(&"/bin/bash".to_string()));

    let report = crate::deps::check(&s, &[crate::deps::Tool::Systemd, crate::deps::Tool::Mtr, crate::deps::Tool::Docker]).await.unwrap();
    assert_eq!(report.tools.iter().map(|t| t.installed).collect::<Vec<_>>(), vec![true, false, true]);
    assert!(report.tools[1].manual_command.as_deref().unwrap().contains("mtr-tiny"));
}

#[tokio::test]
#[ignore]
async fn live_nginx_proxy_host_and_log_analysis() {
    use crate::docker::ExecTarget;
    use crate::nginx::{list_hosts, save_host, NginxTarget, ProxyHost, ProxySsl};
    let (_d, s) = admin_sudo().await;
    let target = NginxTarget { target: ExecTarget::Host, config_root: "/etc/nginx".into() };
    let id = format!("live{}", uuid::Uuid::new_v4().simple());
    let mut host = ProxyHost {
        id: id.clone(),
        domains: vec!["live-test.example.com".into()],
        scheme: "http".into(),
        forward_host: "127.0.0.1".into(),
        forward_port: 8080,
        websockets: true,
        block_exploits: true,
        cache_assets: true,
        ssl: ProxySsl {
            enabled: false,
            certificate: String::new(),
            force_https: false,
            http2: false,
            hsts: false,
            hsts_subdomains: false,
            hsts_preload: false,
        },
        advanced: "client_max_body_size 10m;".into(),
        enabled: true,
    };
    save_host(&s, &target, &host).await.unwrap();
    let listed = list_hosts(&s, &target).await.unwrap();
    assert_eq!(listed.iter().find(|h| h.id == id), Some(&host));
    // nginx really serves it: the request is proxied (and fails with 502, nothing listens on 8080).
    // (A reload is asynchronous: workers pick the new config up a moment later.)
    let code = s
        .run("for i in 1 2 3 4 5 6; do c=$(curl -s -o /dev/null -w '%{http_code}' -H 'Host: live-test.example.com' http://127.0.0.1/); [ \"$c\" = 502 ] && break; sleep 0.5; done; echo $c")
        .await
        .unwrap();
    assert_eq!(code.trim(), "502");

    // A config nginx rejects is rolled back and the previous version stays active.
    let mut broken = host.clone();
    broken.advanced = "this_is_not_a_directive on;".into();
    let err = save_host(&s, &target, &broken).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::ConfigTestFailed);
    assert!(err.details.unwrap().contains("this_is_not_a_directive"));
    assert_eq!(list_hosts(&s, &target).await.unwrap().iter().find(|h| h.id == id), Some(&host));
    assert!(s.run_sudo("nginx -t 2>&1").await.is_ok());

    // Disabling removes it from sites-enabled but keeps the file.
    host.enabled = false;
    save_host(&s, &target, &host).await.unwrap();
    assert!(!list_hosts(&s, &target).await.unwrap().iter().find(|h| h.id == id).unwrap().enabled);

    s.run_sudo(format!("rm -f /etc/nginx/sites-available/jarvis-{id}.conf /etc/nginx/sites-enabled/jarvis-{id}.conf; nginx -s reload"))
        .await
        .unwrap();

    // Access-log analysis on a small generated log (awk pipeline on the server).
    let log = format!("/tmp/jarvis-live-{id}.log");
    let lines = [
        r#"203.0.113.9 - - [03/Oct/2026:20:19:01 +0000] "GET / HTTP/1.1" 200 612 "-" "Mozilla/5.0 (X11; Linux x86_64)""#,
        r#"203.0.113.9 - - [03/Oct/2026:20:20:01 +0000] "GET /api/items?page=2 HTTP/1.1" 200 99 "-" "Mozilla/5.0 (X11; Linux x86_64)""#,
        r#"198.51.100.4 - bob [03/Oct/2026:21:00:00 +0000] "POST /login HTTP/1.1" 401 12 "https://x/" "curl/8.5.0""#,
        r#"2001:db8::1 - - [04/Oct/2026:00:00:10 +0000] "GET /missing HTTP/2.0" 404 0 "-" "-""#,
    ];
    crate::files::write_text(&s, &log, &(lines.join("\n") + "\n")).await.unwrap_or_default();
    s.run(format!("touch {log}")).await.unwrap();
    crate::files::write_text(&s, &log, &(lines.join("\n") + "\n")).await.unwrap();
    let out = s.exec(Exec::new(format!("tail -n 1000 {log} | {}", crate::logs::analysis_pipeline()))).await.unwrap();
    let a = crate::logs::parse_analysis(&out.stdout);
    assert_eq!(a.total, 4, "{}", out.stdout);
    assert_eq!(a.statuses.iter().map(|c| (c.key.as_str(), c.count)).collect::<Vec<_>>(), vec![("200", 2), ("401", 1), ("404", 1)]);
    assert_eq!(a.top_ips[0], crate::logs::Count { key: "203.0.113.9".into(), count: 2 });
    assert!(a.methods.iter().any(|m| m.key == "POST" && m.count == 1));
    assert!(a.top_paths.iter().any(|p| p.key == "/api/items?page=2"));
    assert_eq!(a.top_agents[0].key, "Mozilla/5.0 (X11; Linux x86_64)");
    assert_eq!(a.per_hour.iter().map(|h| h.key.as_str()).collect::<Vec<_>>(), ["2026-10-03 20:00", "2026-10-03 21:00", "2026-10-04 00:00"]);
    s.run(format!("rm -f {log}")).await.unwrap();
}
