//! Docker management: containers, images, networks, volumes, compose, stats.
//!
//! Everything goes through the `docker` CLI with `--format '{{json .}}'`.
//! Whether the CLI needs root is detected once per session and applied
//! transparently.

pub mod compose;
pub mod parse;
pub mod spec;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::jobs::JobMeta;
use crate::shell::{q, validate, Cmd};
use crate::ssh::session::{Exec, Session};
use crate::state::AppState;

pub use parse::*;
pub use spec::ContainerSpec;

/// Whether `docker` needs root on this server (detected once per session).
pub async fn needs_sudo(session: &Session) -> AppResult<bool> {
    let mut cached = session.docker_sudo.lock().await;
    if let Some(value) = *cached {
        return Ok(value);
    }
    let value = if session.is_root() {
        false
    } else {
        let probe = session.exec(Exec::new("docker version --format '{{.Server.Version}}' >/dev/null 2>&1").secs(20)).await?;
        !probe.success()
    };
    *cached = Some(value);
    Ok(value)
}

/// An `Exec` for a docker command line, elevated when this server needs it.
pub async fn exec(session: &Session, script: impl Into<String>) -> AppResult<Exec> {
    Ok(Exec::new(script).sudo_if(needs_sudo(session).await?))
}

/// Run a docker command line and return stdout.
pub async fn run(session: &Session, script: impl Into<String>) -> AppResult<String> {
    session.exec(exec(session, script).await?.secs(60)).await?.into_stdout()
}

fn names(what: &str, values: &[String]) -> AppResult<String> {
    if values.is_empty() {
        return Err(AppError::invalid(format!("No {what} given")));
    }
    let mut quoted = Vec::new();
    for value in values {
        quoted.push(q(validate::name(what, value)?));
    }
    Ok(quoted.join(" "))
}

// ---------------------------------------------------------------- overview

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DockerOverview {
    pub running: u32,
    pub stopped: u32,
    pub images: u32,
    pub networks: u32,
    pub volumes: u32,
    pub version: String,
}

#[tauri::command]
#[specta::specta]
pub async fn docker_overview(state: State<'_, AppState>) -> AppResult<DockerOverview> {
    let session = state.session()?;
    let out = run(
        &session,
        "docker info --format '{{.ContainersRunning}} {{.Containers}} {{.Images}} {{.ServerVersion}}'; \
         docker network ls -q | wc -l; docker volume ls -q | wc -l",
    )
    .await?;
    let mut lines = out.lines();
    let info: Vec<&str> = lines.next().unwrap_or("").split_whitespace().collect();
    let number = |s: Option<&str>| s.and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
    let running = number(info.first().copied());
    Ok(DockerOverview {
        running,
        stopped: number(info.get(1).copied()).saturating_sub(running),
        images: number(info.get(2).copied()),
        version: info.get(3).unwrap_or(&"").to_string(),
        networks: number(lines.next()),
        volumes: number(lines.next()),
    })
}

// ---------------------------------------------------------------- containers

#[tauri::command]
#[specta::specta]
pub async fn docker_containers(state: State<'_, AppState>) -> AppResult<Vec<Container>> {
    let session = state.session()?;
    let out = run(&session, "docker ps -a --no-trunc --format '{{json .}}'").await?;
    Ok(parse_containers(&out))
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
    Kill,
    Pause,
    Unpause,
    Remove,
}

#[tauri::command]
#[specta::specta]
pub async fn docker_container_action(state: State<'_, AppState>, containers: Vec<String>, action: ContainerAction) -> AppResult<()> {
    let session = state.session()?;
    let targets = names("container", &containers)?;
    let verb = match action {
        ContainerAction::Start => "start",
        ContainerAction::Stop => "stop",
        ContainerAction::Restart => "restart",
        ContainerAction::Kill => "kill",
        ContainerAction::Pause => "pause",
        ContainerAction::Unpause => "unpause",
        ContainerAction::Remove => "rm -f",
    };
    session.exec(exec(&session, format!("docker {verb} {targets}")).await?.secs(180)).await?.into_stdout()?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DockerObject {
    Container,
    Image,
    Network,
    Volume,
}

/// Pretty-printed `docker inspect` JSON.
#[tauri::command]
#[specta::specta]
pub async fn docker_inspect(state: State<'_, AppState>, kind: DockerObject, name: String) -> AppResult<String> {
    let session = state.session()?;
    validate::name("name", &name)?;
    let object = match kind {
        DockerObject::Container => "container",
        DockerObject::Image => "image",
        DockerObject::Network => "network",
        DockerObject::Volume => "volume",
    };
    let out = run(&session, format!("docker {object} inspect {}", q(&name))).await?;
    let value: serde_json::Value = serde_json::from_str(&out)?;
    let first = value.get(0).cloned().unwrap_or(value);
    Ok(serde_json::to_string_pretty(&first)?)
}

pub async fn container_detail(session: &Session, name: &str) -> AppResult<ContainerDetail> {
    validate::name("container", name)?;
    let out = run(session, format!("docker container inspect {}", q(name))).await?;
    parse_container_detail(&out)
}

#[tauri::command]
#[specta::specta]
pub async fn docker_container_detail(state: State<'_, AppState>, name: String) -> AppResult<ContainerDetail> {
    let session = state.session()?;
    container_detail(&session, &name).await
}

#[tauri::command]
#[specta::specta]
pub async fn docker_rename(state: State<'_, AppState>, name: String, new_name: String) -> AppResult<()> {
    let session = state.session()?;
    let cmd = Cmd::new("docker").lit("rename").arg(validate::name("container", &name)?).arg(validate::name("container name", &new_name)?);
    run(&session, cmd.build()).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn docker_set_restart_policy(state: State<'_, AppState>, name: String, policy: String) -> AppResult<()> {
    let session = state.session()?;
    let policy = spec::restart_policy(&policy)?;
    let cmd = Cmd::new("docker").lit("update").opt_eq("--restart", policy).arg(validate::name("container", &name)?);
    run(&session, cmd.build()).await?;
    Ok(())
}

/// Connect a container to a network or disconnect it.
#[tauri::command]
#[specta::specta]
pub async fn docker_network_connect(state: State<'_, AppState>, container: String, network: String, connect: bool) -> AppResult<()> {
    let session = state.session()?;
    let cmd = Cmd::new("docker")
        .lit(if connect { "network connect" } else { "network disconnect" })
        .arg(validate::name("network", &network)?)
        .arg(validate::name("container", &container)?);
    run(&session, cmd.build()).await?;
    Ok(())
}

/// Recreate a container with new settings. The old container is renamed and
/// stopped first, and restored if anything goes wrong. Streamed job.
#[tauri::command]
#[specta::specta]
pub async fn docker_recreate(app: AppHandle, state: State<'_, AppState>, name: String, spec: ContainerSpec) -> AppResult<String> {
    let session = state.session()?;
    validate::name("container", &name)?;
    let script = spec::recreate_script(&name, &spec, chrono::Utc::now().timestamp())?;
    let meta = JobMeta::visible(format!("Recreate container {name}"), spec.create_command()?);
    let exec = exec(&session, script).await?;
    state.jobs.start(&app, session, meta, exec).await
}

/// Follow a container's logs as a hidden streamed job.
#[tauri::command]
#[specta::specta]
pub async fn docker_logs_follow(app: AppHandle, state: State<'_, AppState>, name: String, tail: u32) -> AppResult<String> {
    let session = state.session()?;
    validate::name("container", &name)?;
    let script = format!("docker logs -f --tail {} {} 2>&1", tail.clamp(1, 100_000), q(&name));
    let exec = exec(&session, script).await?;
    state.jobs.start(&app, session, JobMeta::hidden(format!("Logs · {name}")), exec).await
}

/// Follow the Docker event stream as a hidden job, one `<type> <action>` line per event, so
/// the UI notices changes made outside Jarvis (a terminal, another client).
#[tauri::command]
#[specta::specta]
pub async fn docker_events_follow(app: AppHandle, state: State<'_, AppState>) -> AppResult<String> {
    let session = state.session()?;
    let exec = exec(&session, "docker events --format '{{.Type}} {{.Action}}'").await?;
    state.jobs.start(&app, session, JobMeta::hidden("Docker events"), exec).await
}

/// Run a one-off command (given by the user) inside a container.
#[tauri::command]
#[specta::specta]
pub async fn docker_exec(state: State<'_, AppState>, name: String, command: String) -> AppResult<String> {
    let session = state.session()?;
    validate::name("container", &name)?;
    if command.trim().is_empty() {
        return Err(AppError::invalid("Enter a command"));
    }
    let cmd = Cmd::new("docker").lit("exec").arg(&name).lit("sh -c").arg(&command).lit("2>&1");
    let out = session.exec(exec(&session, cmd.build()).await?.secs(120)).await?;
    let mut text = out.stdout;
    if out.code != 0 {
        text.push_str(&format!("\n[exit code {}]", out.code));
    }
    Ok(text)
}

// ---------------------------------------------------------------- images

#[tauri::command]
#[specta::specta]
pub async fn docker_images(state: State<'_, AppState>) -> AppResult<Vec<Image>> {
    let session = state.session()?;
    let out = run(
        &session,
        "docker images --no-trunc --format '{{json .}}'; echo '#used'; \
         ids=$(docker ps -aq); [ -n \"$ids\" ] && docker inspect --format '{{.Image}}' $ids; true",
    )
    .await?;
    let (images, used) = out.split_once("#used\n").unwrap_or((&out, ""));
    Ok(parse_images(images, used))
}

#[tauri::command]
#[specta::specta]
pub async fn docker_image_remove(state: State<'_, AppState>, images: Vec<String>, force: bool) -> AppResult<()> {
    let session = state.session()?;
    let targets = names("image", &images)?;
    let flag = if force { "-f " } else { "" };
    run(&session, format!("docker rmi {flag}{targets}")).await?;
    Ok(())
}

/// Pull an image with live progress (visible job).
#[tauri::command]
#[specta::specta]
pub async fn docker_image_pull(app: AppHandle, state: State<'_, AppState>, image: String) -> AppResult<String> {
    let session = state.session()?;
    let script = format!("docker pull {}", q(validate::name("image", image.trim())?));
    let exec = exec(&session, script.clone()).await?;
    state.jobs.start(&app, session, JobMeta::visible(format!("Pull {}", image.trim()), script), exec).await
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PruneTarget {
    Images,
    Volumes,
    Containers,
    Networks,
}

/// Remove unused objects; returns docker's summary.
#[tauri::command]
#[specta::specta]
pub async fn docker_prune(state: State<'_, AppState>, target: PruneTarget) -> AppResult<String> {
    let session = state.session()?;
    let script = match target {
        PruneTarget::Images => "docker image prune -a -f",
        PruneTarget::Volumes => "docker volume prune -f",
        PruneTarget::Containers => "docker container prune -f",
        PruneTarget::Networks => "docker network prune -f",
    };
    session.exec(exec(&session, script).await?.secs(300)).await?.into_stdout()
}

// ---------------------------------------------------------------- networks & volumes

#[tauri::command]
#[specta::specta]
pub async fn docker_networks(state: State<'_, AppState>) -> AppResult<Vec<Network>> {
    let session = state.session()?;
    Ok(parse_networks(&run(&session, "docker network ls --no-trunc --format '{{json .}}'").await?))
}

#[tauri::command]
#[specta::specta]
pub async fn docker_network_create(state: State<'_, AppState>, name: String, driver: String) -> AppResult<()> {
    let session = state.session()?;
    let driver = validate::one_of("driver", &driver, &["bridge", "overlay", "macvlan", "ipvlan", "host", "none"])?;
    let cmd = Cmd::new("docker").lit("network create").opt("--driver", driver).arg(validate::name("network name", &name)?);
    run(&session, cmd.build()).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn docker_network_remove(state: State<'_, AppState>, networks: Vec<String>) -> AppResult<()> {
    let session = state.session()?;
    run(&session, format!("docker network rm {}", names("network", &networks)?)).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn docker_volumes(state: State<'_, AppState>) -> AppResult<Vec<Volume>> {
    let session = state.session()?;
    let out = run(
        &session,
        "docker volume ls --format '{{json .}}'; echo '#used'; \
         ids=$(docker ps -aq); [ -n \"$ids\" ] && docker inspect --format '{{range .Mounts}}{{println .Name}}{{end}}' $ids; true",
    )
    .await?;
    let (volumes, used) = out.split_once("#used\n").unwrap_or((&out, ""));
    Ok(parse_volumes(volumes, used))
}

#[tauri::command]
#[specta::specta]
pub async fn docker_volume_remove(state: State<'_, AppState>, volumes: Vec<String>) -> AppResult<()> {
    let session = state.session()?;
    run(&session, format!("docker volume rm {}", names("volume", &volumes)?)).await?;
    Ok(())
}

// ---------------------------------------------------------------- stats

#[tauri::command]
#[specta::specta]
pub async fn docker_stats(state: State<'_, AppState>) -> AppResult<Vec<ContainerStats>> {
    let session = state.session()?;
    let out = run(&session, "docker stats --no-stream --no-trunc --format '{{json .}}'").await?;
    Ok(parse_stats(&out))
}

// ---------------------------------------------------------------- execution targets

/// Where a tool runs: directly on the host or inside a Docker container.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExecTarget {
    Host,
    Container { container: String },
}

impl ExecTarget {
    pub fn is_host(&self) -> bool {
        matches!(self, ExecTarget::Host)
    }
}

/// An `Exec` that runs `script` in the target. On the host, `root` selects
/// sudo; in a container the script runs as the container's user through
/// `docker exec` (with `-i`, so stdin reaches the script).
pub async fn in_target(session: &Session, target: &ExecTarget, script: &str, root: bool) -> AppResult<Exec> {
    match target {
        ExecTarget::Host => Ok(Exec::new(script).sudo_if(root)),
        ExecTarget::Container { container } => {
            let line = Cmd::new("docker").lit("exec -i").arg(validate::name("container", container)?).lit("sh -c").arg(script).build();
            exec(session, line).await
        }
    }
}
