//! End-to-end checks of the generated backup scripts and restic parsing
//! against the disposable test server (see `live_tests.rs`).

use crate::backups::*;
use crate::restic::{env_exports, parse_nodes, parse_snapshots, KeepPolicy, ResticRepo};
use crate::shell::q;
use crate::ssh::session::{Exec, Session};

fn template() -> BackupTemplate {
    BackupTemplate {
        id: "abc123".into(),
        name: "Live test".into(),
        kind: "files".into(),
        path: "/srv/app".into(),
        db_source: "host".into(),
        db_host: "127.0.0.1".into(),
        db_port: 3306,
        db_container: String::new(),
        db_name: "shop".into(),
        db_user: "shop".into(),
        destination: "folder".into(),
        folder: "/var/backups/app".into(),
        s3_endpoint: String::new(),
        s3_region: String::new(),
        s3_bucket: String::new(),
        s3_prefix: String::new(),
        sftp_host: String::new(),
        sftp_port: 22,
        sftp_user: String::new(),
        sftp_path: String::new(),
        restic_repo: String::new(),
        schedule: String::new(),
        paused: false,
        sudo: false,
        keep_days: 0,
        keep: KeepPolicy::default(),
    }
}

fn wrap(script: &str) -> String {
    format!("eval \"$(cat)\"\n{{\n{script}}} 2>&1")
}

async fn run(session: &Session, script: &str, env: &[(String, String)], sudo: bool) -> String {
    let exec = Exec::new(wrap(script)).stdin(env_exports(env)).sudo_if(sudo).secs(180);
    let out = session.exec(exec).await.unwrap();
    assert!(out.success(), "script failed:\n{}", out.stdout);
    out.stdout
}

/// Files and both database engines to a folder, then files into a restic repository.
#[tokio::test]
#[ignore]
async fn live_backup_scripts_and_restic() {
    let (_d, s) = crate::live_tests::keyuser().await;
    let root = format!("/tmp/jarvis-live-{}", uuid::Uuid::new_v4().simple());
    s.run(format!("mkdir -p {root}/src/sub {root}/out && echo hello > {root}/src/a.txt && echo deep > {root}/src/sub/b.txt"))
        .await
        .unwrap();
    let none = RunOptions { download_to: None, chown: None, restic: None, restic_as: None };

    // Files → folder (as root, with retention that must not delete the fresh archive).
    let mut t = template();
    t.path = format!("{root}/src");
    t.folder = format!("{root}/out");
    t.keep_days = 7;
    let out = run(&s, &build_script(&t, &none).unwrap(), &[], true).await;
    assert!(out.contains("Backup finished."), "{out}");
    let listing = s.run_sudo(format!("tar -tzf {root}/out/live-test-*.tar.gz")).await.unwrap();
    assert!(listing.contains("src/sub/b.txt"), "{listing}");

    // MySQL and PostgreSQL dumps; the password only travels in the environment.
    for (kind, host, port, user, password, db, marker) in [
        ("mysql", "jarvis-test-mysql", 3306, "shop", "shoppw", "shop", "Dump completed"),
        ("postgres", "jarvis-test-postgres", 5432, "app", "pgpw", "appdb", "PostgreSQL database dump"),
    ] {
        let mut t = template();
        t.kind = kind.into();
        t.db_host = host.into();
        // The server's pg_dump is older than the PostgreSQL container: dump inside the container.
        let in_container = kind == "postgres";
        if in_container {
            t.db_source = "container".into();
            t.db_container = host.into();
        }
        t.db_port = port;
        t.db_user = user.into();
        t.db_name = db.into();
        t.folder = format!("{root}/dumps");
        let script = build_script(&t, &none).unwrap();
        run(&s, &script, &[("DB_PASSWORD".to_string(), password.to_string())], in_container).await;
        let dump = s.run_sudo(format!("gzip -dc {root}/dumps/live-test-*.sql.gz | head -c 20000; rm -rf {root}/dumps")).await.unwrap();
        assert!(dump.contains(marker), "{kind} dump looks wrong:\n{dump}");
        // A wrong password fails the run instead of leaving an empty archive behind.
        // (Inside the PostgreSQL container the local socket is trusted, so there is nothing to fail.)
        if !in_container {
            let bad = [("DB_PASSWORD".to_string(), "wrong".to_string())];
            let out = s.exec(Exec::new(wrap(&script)).stdin(env_exports(&bad)).sudo_if(in_container)).await.unwrap();
            assert!(!out.success(), "{}", out.stdout);
            assert_eq!(s.run_sudo(format!("ls {root}/dumps 2>/dev/null | wc -l")).await.unwrap().trim(), "0");
        }
    }

    // Restic: init, back up through the template, list, browse, apply retention.
    let repo = ResticRepo {
        id: "live".into(),
        name: "live".into(),
        kind: "local".into(),
        repository: format!("{root}/repo"),
        fields: Default::default(),
        env_names: vec![],
        sudo: false,
    };
    let env =
        vec![("RESTIC_REPOSITORY".to_string(), repo.repository.clone()), ("RESTIC_PASSWORD".to_string(), "it's a $ecret".to_string())];
    run(&s, "restic init\n", &env, false).await;
    let mut t = template();
    t.path = format!("{root}/src");
    t.destination = "restic".into();
    t.restic_repo = "live".into();
    t.keep = KeepPolicy { last: 1, ..Default::default() };
    let options = RunOptions { download_to: None, chown: None, restic: Some(&repo), restic_as: None };
    let script = build_script(&t, &options).unwrap();
    run(&s, &script, &env, false).await;
    run(&s, &script, &env, false).await;
    let snapshots = parse_snapshots(&run(&s, "restic snapshots --json 2>/dev/null\n", &env, false).await).unwrap();
    assert_eq!(snapshots.len(), 1, "keep-last 1 leaves one snapshot");
    assert_eq!(snapshots[0].tags, vec!["jarvis-abc123".to_string()]);
    let id = &snapshots[0].id;
    let listing = run(&s, &format!("restic ls --json {id} {root}/src 2>/dev/null\n"), &env, false).await;
    let nodes = parse_nodes(&listing, Some(&format!("{root}/src")));
    assert_eq!(nodes.iter().map(|n| (n.name.as_str(), n.dir)).collect::<Vec<_>>(), [("sub", true), ("a.txt", false)], "{listing}");
    let found = run(&s, &format!("restic find --json -i -s {id} '*B.tx*' 2>/dev/null\n"), &env, false).await;
    let found = parse_nodes(&found, None);
    assert_eq!(found.len(), 1);
    assert!(found[0].path.ends_with("/src/sub/b.txt"));
    let text = run(&s, &format!("restic dump {id} {root}/src/a.txt 2>/dev/null\n"), &env, false).await;
    assert_eq!(text, "hello\n");
    let stats = run(&s, "restic stats --json --mode raw-data 2>/dev/null\n", &env, false).await;
    assert!(crate::restic::parse_stats(&stats).unwrap().total_size > 0);
    // A wrong password is a failure, not an empty list.
    let bad = [env[0].clone(), ("RESTIC_PASSWORD".to_string(), "nope".to_string())];
    let out = s.exec(Exec::new("eval \"$(cat)\"; restic snapshots --json").stdin(env_exports(&bad))).await.unwrap();
    assert!(!out.success());

    // The scheduled wrapper: env file, framed log, exit code.
    let mut t = template();
    t.id = format!("live{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    t.path = format!("{root}/src");
    t.folder = format!("{root}/out");
    let file = scheduled_script(&t, &build_script(&t, &none).unwrap());
    let (script, env_file) = (format!("/usr/local/bin/jarvis-backup-{}.sh", t.id), format!("/etc/jarvis-backups/{}.env", t.id));
    let install = format!(
        "mkdir -p /etc/jarvis-backups && : > {env_file} && chmod 600 {env_file} && printf '%s' {} > {script} && chmod 755 {script} && {script}; code=$?; rm -f {script} {env_file}; exit $code",
        q(&file),
    );
    let out = s.exec(Exec::new(install).sudo()).await.unwrap();
    assert!(out.success(), "{}", out.combined());
    assert!(out.stdout.contains("backup started ===") && out.stdout.contains("backup finished (exit 0) ==="), "{}", out.stdout);

    s.run_sudo(format!("rm -rf {root}")).await.unwrap();
}
