//! Users and groups.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::shell::{q, validate};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub comment: String,
    pub home: String,
    pub shell: String,
    /// All groups: the primary one first, then supplementary groups.
    pub groups: Vec<String>,
    /// `None` when it could not be determined without root.
    pub locked: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub name: String,
    pub gid: u32,
    /// Explicit members plus users whose primary group this is.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Accounts {
    pub users: Vec<User>,
    pub groups: Vec<Group>,
    /// The user Jarvis is connected as (cannot be deleted).
    pub current_user: String,
    pub shells: Vec<String>,
}

/// Parse `getent passwd`, `getent group` and (optionally) `passwd -S -a` output.
pub fn parse_accounts(passwd: &str, group: &str, status: &str) -> (Vec<User>, Vec<Group>) {
    let mut groups: Vec<Group> = group
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            (f.len() >= 4).then(|| Group {
                name: f[0].to_string(),
                gid: f[2].parse().unwrap_or(0),
                members: f[3].split(',').filter(|m| !m.is_empty()).map(str::to_string).collect(),
            })
        })
        .collect();
    let by_gid: BTreeMap<u32, String> = groups.iter().map(|g| (g.gid, g.name.clone())).collect();
    // `passwd -S`: "name L 2024-01-01 0 99999 7 -1" (L/LK locked, P/PS usable, NP no password).
    let locked: BTreeMap<&str, bool> = status
        .lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace();
            let name = f.next()?;
            let flag = f.next()?;
            Some((name, flag.starts_with('L')))
        })
        .collect();

    let users: Vec<User> = passwd
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() < 7 {
                return None;
            }
            let name = f[0].to_string();
            let gid: u32 = f[3].parse().ok()?;
            let mut user_groups: Vec<String> = by_gid.get(&gid).cloned().into_iter().collect();
            for g in &groups {
                if g.members.contains(&name) && !user_groups.contains(&g.name) {
                    user_groups.push(g.name.clone());
                }
            }
            Some(User {
                uid: f[2].parse().ok()?,
                gid,
                comment: f[4].split(',').next().unwrap_or("").to_string(),
                home: f[5].to_string(),
                shell: f[6].to_string(),
                groups: user_groups,
                locked: locked.get(name.as_str()).copied(),
                name,
            })
        })
        .collect();

    for user in &users {
        if let Some(primary) = groups.iter_mut().find(|g| g.gid == user.gid) {
            if !primary.members.contains(&user.name) {
                primary.members.push(user.name.clone());
            }
        }
    }
    (users, groups)
}

pub async fn list(session: &Session) -> AppResult<Accounts> {
    const SECTION: &str = "#jarvis-section";
    let base = session
        .run(format!(
            "getent passwd 2>/dev/null || cat /etc/passwd; echo '{SECTION}'; getent group 2>/dev/null || cat /etc/group; \
             echo '{SECTION}'; grep -v '^#' /etc/shells 2>/dev/null; true"
        ))
        .await?;
    // Lock state needs root; include it only when that will not prompt.
    let status = if session.sudo_ready().await {
        session.exec(Exec::new("passwd -S -a 2>/dev/null; true").sudo()).await.map(|o| o.stdout).unwrap_or_default()
    } else {
        String::new()
    };
    let parts: Vec<&str> = base.split(SECTION).collect();
    let (users, groups) = parse_accounts(parts.first().unwrap_or(&""), parts.get(1).unwrap_or(&""), &status);
    let mut shells: Vec<String> =
        parts.get(2).unwrap_or(&"").lines().map(str::trim).filter(|l| l.starts_with('/')).map(str::to_string).collect();
    if shells.is_empty() {
        shells = vec!["/bin/sh".into(), "/bin/bash".into()];
    }
    Ok(Accounts { users, groups, current_user: session.facts.user.clone(), shells })
}

async fn root(session: &Session, script: String) -> AppResult<()> {
    session.exec(Exec::new(script).sudo().secs(60)).await?.into_stdout()?;
    Ok(())
}

fn protect(session: &Session, user: &str, action: &str) -> AppResult<()> {
    if user == "root" {
        return Err(AppError::invalid(format!("The root account cannot be {action}")));
    }
    if user == session.facts.user {
        return Err(AppError::invalid(format!("The account Jarvis is connected with cannot be {action}")));
    }
    Ok(())
}

// ---------------------------------------------------------------- commands

#[tauri::command]
#[specta::specta]
pub async fn accounts_list(state: State<'_, AppState>) -> AppResult<Accounts> {
    let session = state.session()?;
    list(&session).await
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewUser {
    pub name: String,
    pub shell: String,
    pub comment: String,
    /// Optional initial password (sent on stdin to `chpasswd`).
    pub password: String,
}

#[tauri::command]
#[specta::specta]
pub async fn user_create(state: State<'_, AppState>, user: NewUser) -> AppResult<()> {
    let session = state.session()?;
    let name = validate::username(&user.name)?;
    let shell = validate::abs_path(user.shell.trim())?;
    let comment = validate::single_line("full name", user.comment.trim())?;
    if comment.contains(':') {
        return Err(AppError::invalid("The full name cannot contain “:”"));
    }
    let (n, s, c) = (q(name), q(shell), q(comment));
    // BusyBox systems (Alpine) only have `adduser`.
    root(
        &session,
        format!(
            "if command -v useradd >/dev/null 2>&1; then useradd -m -s {s} -c {c} {n}; \
             else adduser -D -s {s} -g {c} {n}; fi"
        ),
    )
    .await?;
    if !user.password.is_empty() {
        set_password(&session, name, &user.password).await?;
    }
    Ok(())
}

async fn set_password(session: &Session, name: &str, password: &str) -> AppResult<()> {
    if password.contains('\n') || password.contains(':') && false {
        return Err(AppError::invalid("The password cannot contain line breaks"));
    }
    session.exec(Exec::new("chpasswd").sudo().stdin(format!("{name}:{password}\n"))).await?.into_stdout()?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn user_delete(state: State<'_, AppState>, name: String, remove_home: bool) -> AppResult<()> {
    let session = state.session()?;
    let name = validate::username(&name)?;
    protect(&session, name, "deleted")?;
    let n = q(name);
    let (userdel_flag, deluser_flag) = if remove_home { ("-r ", "--remove-home ") } else { ("", "") };
    root(&session, format!("if command -v userdel >/dev/null 2>&1; then userdel {userdel_flag}{n}; else deluser {deluser_flag}{n}; fi"))
        .await
}

/// Change a password. It travels on stdin to `chpasswd`, never on a command line.
#[tauri::command]
#[specta::specta]
pub async fn user_set_password(state: State<'_, AppState>, name: String, password: String) -> AppResult<()> {
    let session = state.session()?;
    let name = validate::username(&name)?;
    if password.is_empty() {
        return Err(AppError::invalid("Enter a password"));
    }
    set_password(&session, name, &password).await
}

#[tauri::command]
#[specta::specta]
pub async fn user_set_locked(state: State<'_, AppState>, name: String, locked: bool) -> AppResult<()> {
    let session = state.session()?;
    let name = validate::username(&name)?;
    if locked {
        protect(&session, name, "locked")?;
    }
    let flag = if locked { "-l" } else { "-u" };
    root(&session, format!("passwd {flag} {}", q(name))).await
}

/// Set a user's supplementary groups by adding and removing individual
/// memberships, so nothing else about the account is touched.
#[tauri::command]
#[specta::specta]
pub async fn user_set_groups(state: State<'_, AppState>, name: String, groups: Vec<String>) -> AppResult<()> {
    let session = state.session()?;
    let name = validate::username(&name)?;
    for g in &groups {
        validate::username(g)?;
    }
    let accounts = list(&session).await?;
    let user = accounts.users.iter().find(|u| u.name == name).ok_or_else(|| AppError::invalid("Unknown user"))?;
    let primary = accounts.groups.iter().find(|g| g.gid == user.gid).map(|g| g.name.as_str());
    let current: Vec<&str> = user.groups.iter().map(String::as_str).filter(|g| Some(*g) != primary).collect();
    let add: Vec<&String> = groups.iter().filter(|g| !current.contains(&g.as_str()) && Some(g.as_str()) != primary).collect();
    let remove: Vec<&&str> = current.iter().filter(|g| !groups.iter().any(|w| w == *g)).collect();
    if name == session.facts.user && remove.iter().any(|g| matches!(**g, "sudo" | "wheel")) {
        return Err(AppError::invalid("Removing your own account from the sudo group would lock Jarvis out"));
    }
    if add.is_empty() && remove.is_empty() {
        return Ok(());
    }
    let n = q(name);
    let mut script = String::from("set -e\n");
    for g in add {
        let g = q(g);
        script.push_str(&format!("if command -v gpasswd >/dev/null 2>&1; then gpasswd -a {n} {g}; else addgroup {n} {g}; fi\n"));
    }
    for g in remove {
        let g = q(g);
        script.push_str(&format!("if command -v gpasswd >/dev/null 2>&1; then gpasswd -d {n} {g}; else delgroup {n} {g}; fi\n"));
    }
    root(&session, script).await
}

#[tauri::command]
#[specta::specta]
pub async fn group_create(state: State<'_, AppState>, name: String) -> AppResult<()> {
    let session = state.session()?;
    let g = q(validate::username(&name)?);
    root(&session, format!("if command -v groupadd >/dev/null 2>&1; then groupadd {g}; else addgroup {g}; fi")).await
}

#[tauri::command]
#[specta::specta]
pub async fn group_delete(state: State<'_, AppState>, name: String) -> AppResult<()> {
    let session = state.session()?;
    let name = validate::username(&name)?;
    if matches!(name, "root" | "sudo" | "wheel") {
        return Err(AppError::invalid("This group is essential and cannot be deleted here"));
    }
    let g = q(name);
    root(&session, format!("if command -v groupdel >/dev/null 2>&1; then groupdel {g}; else delgroup {g}; fi")).await
}

async fn home_of(session: &Session, name: &str) -> AppResult<String> {
    let out = session.run(format!("getent passwd {n} 2>/dev/null || grep '^'{n}: /etc/passwd", n = q(name))).await?;
    out.lines()
        .next()
        .and_then(|l| l.split(':').nth(5))
        .filter(|h| h.starts_with('/'))
        .map(str::to_string)
        .ok_or_else(|| AppError::invalid("Unknown user"))
}

/// A user's `authorized_keys` (empty if the file does not exist).
#[tauri::command]
#[specta::specta]
pub async fn user_keys_get(state: State<'_, AppState>, name: String) -> AppResult<String> {
    let session = state.session()?;
    let name = validate::username(&name)?;
    let file = format!("{}/.ssh/authorized_keys", home_of(&session, name).await?.trim_end_matches('/'));
    let out = session.exec_auto(Exec::new(format!("f={}; [ -e \"$f\" ] || exit 0; cat -- \"$f\"", q(&file)))).await?;
    out.into_stdout()
}

#[tauri::command]
#[specta::specta]
pub async fn user_keys_set(state: State<'_, AppState>, name: String, content: String) -> AppResult<()> {
    let session = state.session()?;
    let name = validate::username(&name)?;
    let home = home_of(&session, name).await?;
    let dir = format!("{}/.ssh", home.trim_end_matches('/'));
    let (d, n) = (q(&dir), q(name));
    let mut content = content.replace("\r\n", "\n");
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    session
        .exec(
            Exec::new(format!(
                "mkdir -p {d} && cat > {d}/authorized_keys && chmod 700 {d} && chmod 600 {d}/authorized_keys && \
                 chown -R {n}: {d}"
            ))
            .sudo()
            .stdin(content),
        )
        .await?
        .into_stdout()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\n\
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\n\
deploy:x:1000:1000:Deploy User,,,:/home/deploy:/bin/bash\n\
app:x:1001:1001::/srv/app:/bin/sh\n";
    const GROUP: &str = "root:x:0:\ndaemon:x:1:\nsudo:x:27:deploy\ndocker:x:998:deploy,app\ndeploy:x:1000:\napp:x:1001:\n";

    #[test]
    fn parses_users_groups_and_membership() {
        let (users, groups) = parse_accounts(PASSWD, GROUP, "");
        assert_eq!(users.len(), 4);
        let deploy = &users[2];
        assert_eq!(deploy.uid, 1000);
        assert_eq!(deploy.comment, "Deploy User");
        assert_eq!(deploy.groups, vec!["deploy", "sudo", "docker"]);
        assert_eq!(deploy.locked, None);
        assert_eq!(users[3].groups, vec!["app", "docker"]);

        let docker = groups.iter().find(|g| g.name == "docker").unwrap();
        assert_eq!(docker.members, vec!["deploy", "app"]);
        // Primary-group members are listed too.
        let deploy_group = groups.iter().find(|g| g.name == "deploy").unwrap();
        assert_eq!(deploy_group.members, vec!["deploy"]);
    }

    #[test]
    fn lock_status_from_passwd_s() {
        let status =
            "root P 2024-01-01 0 99999 7 -1\ndeploy L 2024-05-05 0 99999 7 -1\napp LK 2024-05-05 0 99999 7 -1 (Password locked.)\n";
        let (users, _) = parse_accounts(PASSWD, GROUP, status);
        assert_eq!(users[0].locked, Some(false));
        assert_eq!(users[1].locked, None);
        assert_eq!(users[2].locked, Some(true));
        assert_eq!(users[3].locked, Some(true));
    }

    #[test]
    fn tolerates_malformed_lines() {
        let (users, groups) = parse_accounts("broken line\nx:y\n", "nope\n", "");
        assert!(users.is_empty() && groups.is_empty());
    }
}
