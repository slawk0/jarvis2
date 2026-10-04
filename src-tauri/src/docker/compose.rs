//! Docker Compose projects.
//!
//! `docker compose ls` only knows projects that still have containers. Every
//! project Jarvis sees is remembered per profile, so a stack that was brought
//! down stays listed and can be brought up again.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use super::exec;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::files;
use crate::jobs::JobMeta;
use crate::shell::{q, validate};
use crate::ssh::session::Exec;
use crate::state::AppState;
use crate::store::profile_data::DataKey;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RememberedStack {
    pub name: String,
    pub config_file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ComposeProject {
    pub name: String,
    /// e.g. "running(2)", "exited(1)"; empty when the stack is down.
    pub status: String,
    pub config_file: String,
    pub running: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LsEntry {
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    config_files: String,
}

/// Merge `docker compose ls --all --format json` with the remembered stacks.
pub fn merge_projects(ls_json: &str, remembered: &[RememberedStack]) -> Vec<ComposeProject> {
    let listed: Vec<LsEntry> = serde_json::from_str(ls_json.trim()).unwrap_or_default();
    let mut projects: Vec<ComposeProject> = listed
        .into_iter()
        .map(|e| ComposeProject {
            running: e.status.contains("running"),
            // Several files can be listed (overrides); the first is the main one.
            config_file: e.config_files.split(',').next().unwrap_or("").trim().to_string(),
            name: e.name,
            status: e.status,
        })
        .collect();
    for stack in remembered {
        let known = projects.iter().any(|p| p.config_file == stack.config_file || p.name == stack.name);
        if !known {
            projects.push(ComposeProject {
                name: stack.name.clone(),
                status: String::new(),
                config_file: stack.config_file.clone(),
                running: false,
            });
        }
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    projects
}

fn compose_file(path: &str) -> AppResult<&str> {
    validate::abs_path(path)?;
    let name = path.rsplit('/').next().unwrap_or("");
    if name.ends_with(".yml") || name.ends_with(".yaml") {
        Ok(path)
    } else {
        Err(AppError::invalid("Not a compose file (.yml or .yaml)"))
    }
}

/// `docker compose -f <file> --project-directory <dir>`.
fn base(config_file: &str) -> AppResult<String> {
    let file = compose_file(config_file)?;
    Ok(format!("docker compose -f {} --project-directory {}", q(file), q(&files::parent(file))))
}

#[tauri::command]
#[specta::specta]
pub async fn compose_list(state: State<'_, AppState>) -> AppResult<Vec<ComposeProject>> {
    let session = state.session()?;
    let out = super::run(&session, "docker compose ls --all --format json").await?;
    let profile = session.profile.id.clone();
    let remembered: Vec<RememberedStack> = state.data.get_as(&profile, DataKey::ComposeStacks)?;
    let projects = merge_projects(&out, &remembered);
    // Remember everything seen so stacks that go down stay listed.
    let all: Vec<RememberedStack> = projects
        .iter()
        .filter(|p| !p.config_file.is_empty())
        .map(|p| RememberedStack { name: p.name.clone(), config_file: p.config_file.clone() })
        .collect();
    if all != remembered {
        state.data.set(&profile, DataKey::ComposeStacks, &all)?;
    }
    Ok(projects)
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ComposeAction {
    Up,
    Down,
    Restart,
    Pull,
}

/// Run a compose action as a visible streamed job.
#[tauri::command]
#[specta::specta]
pub async fn compose_action(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    config_file: String,
    action: ComposeAction,
) -> AppResult<String> {
    let session = state.session()?;
    let (verb, title) = match action {
        ComposeAction::Up => ("up -d --remove-orphans", "Compose up"),
        ComposeAction::Down => ("down", "Compose down"),
        ComposeAction::Restart => ("restart", "Compose restart"),
        ComposeAction::Pull => ("pull", "Compose pull"),
    };
    let script = format!("{} {verb} 2>&1", base(&config_file)?);
    let exec = exec(&session, script.clone()).await?;
    state.jobs.start(&app, session, JobMeta::visible(format!("{title} · {name}"), script), exec).await
}

/// Follow a project's logs as a hidden streamed job.
#[tauri::command]
#[specta::specta]
pub async fn compose_logs(app: AppHandle, state: State<'_, AppState>, name: String, config_file: String) -> AppResult<String> {
    let session = state.session()?;
    let script = format!("{} logs -f --tail 200 2>&1", base(&config_file)?);
    let exec = exec(&session, script).await?;
    state.jobs.start(&app, session, JobMeta::hidden(format!("Compose logs · {name}")), exec).await
}

/// Validate a compose file with `docker compose config`; returns the error text, if any.
#[tauri::command]
#[specta::specta]
pub async fn compose_validate(state: State<'_, AppState>, config_file: String) -> AppResult<Option<String>> {
    let session = state.session()?;
    let script = format!("{} config -q 2>&1", base(&config_file)?);
    let out = session.exec(exec(&session, script).await?.secs(60)).await?;
    Ok((!out.success()).then(|| out.stdout.trim().to_string()))
}

/// Stop listing a stack (its files and containers are left alone).
#[tauri::command]
#[specta::specta]
pub async fn compose_forget(state: State<'_, AppState>, config_file: String) -> AppResult<()> {
    let session = state.session()?;
    state.data.update(&session.profile.id, DataKey::ComposeStacks, |stacks: &mut Vec<RememberedStack>| {
        stacks.retain(|s| s.config_file != config_file)
    })
}

/// Create a new project folder with a starter compose file.
#[tauri::command]
#[specta::specta]
pub async fn compose_create(state: State<'_, AppState>, directory: String, folder: String, content: String) -> AppResult<String> {
    let session = state.session()?;
    validate::abs_path(&directory)?;
    let name = validate::slug("folder name", &folder)?.to_lowercase();
    let dir = files::join(&directory, &folder);
    let file = format!("{dir}/docker-compose.yml");
    let exists = session.exec_auto(Exec::new(format!("[ -e {} ]", q(&dir)))).await?.success();
    if exists {
        return Err(AppError::new(ErrorCode::AlreadyExists, dir));
    }
    session.exec_auto(Exec::new(format!("mkdir -p -- {d} && cat > {f}", d = q(&dir), f = q(&file))).stdin(content)).await?.into_stdout()?;
    state.data.update(&session.profile.id, DataKey::ComposeStacks, |stacks: &mut Vec<RememberedStack>| {
        stacks.push(RememberedStack { name, config_file: file.clone() })
    })?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_listed_and_remembered_projects() {
        let ls = r#"[{"Name":"blog","Status":"running(2)","ConfigFiles":"/srv/blog/docker-compose.yml"},
                     {"Name":"mail","Status":"exited(3)","ConfigFiles":"/srv/mail/compose.yaml,/srv/mail/compose.override.yaml"}]"#;
        let remembered = vec![
            RememberedStack { name: "blog".into(), config_file: "/srv/blog/docker-compose.yml".into() },
            RememberedStack { name: "wiki".into(), config_file: "/srv/wiki/docker-compose.yml".into() },
        ];
        let p = merge_projects(ls, &remembered);
        let names: Vec<&str> = p.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["blog", "mail", "wiki"]);
        assert!(p[0].running);
        assert!(!p[1].running);
        assert_eq!(p[1].config_file, "/srv/mail/compose.yaml");
        // The stack that is down is still listed, from memory.
        assert_eq!((p[2].status.as_str(), p[2].running), ("", false));
    }

    #[test]
    fn tolerates_empty_or_invalid_ls_output() {
        assert!(merge_projects("", &[]).is_empty());
        assert!(merge_projects("[]", &[]).is_empty());
        assert_eq!(merge_projects("garbage", &[RememberedStack { name: "a".into(), config_file: "/a/compose.yml".into() }]).len(), 1);
    }

    #[test]
    fn compose_command_base() {
        assert_eq!(
            base("/srv/my app/docker-compose.yml").unwrap(),
            "docker compose -f '/srv/my app/docker-compose.yml' --project-directory '/srv/my app'"
        );
        assert!(base("/etc/passwd").is_err());
        assert!(base("relative.yml").is_err());
    }
}
