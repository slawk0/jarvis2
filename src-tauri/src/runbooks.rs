//! Runbooks: named, user-authored commands. Together with the terminal this
//! is the only place where an arbitrary command from the user is executed.

use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::jobs::JobMeta;
use crate::ssh::session::Exec;
use crate::state::AppState;

/// Run a runbook as a streamed job; returns the job id.
#[tauri::command]
#[specta::specta]
pub async fn runbook_run(app: AppHandle, state: State<'_, AppState>, name: String, command: String, sudo: bool) -> AppResult<String> {
    if command.trim().is_empty() {
        return Err(AppError::invalid("The runbook has no command"));
    }
    let session = state.session()?;
    let meta = JobMeta::visible(format!("Runbook · {name}"), command.clone());
    state.jobs.start(&app, session, meta, Exec::new(command).sudo_if(sudo)).await
}
