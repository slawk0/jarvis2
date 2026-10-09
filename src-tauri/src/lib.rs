//! Jarvis Server Manager backend.
//!
//! One module per domain. Commands take validated arguments and return parsed,
//! typed data; the frontend never builds shell strings.

pub mod backups;
#[cfg(test)]
mod backups_live;
pub mod connection;
pub mod cron;
pub mod crowdsec;
pub mod db;
pub mod deps;
pub mod disks;
pub mod docker;
pub mod env;
pub mod error;
pub mod files;
pub mod firewall;
pub mod jobs;
#[cfg(test)]
mod live_tests;
pub mod local;
pub mod logs;
pub mod network;
pub mod nginx;
pub mod packages;
pub mod pangolin;
pub mod restic;
pub mod runbooks;
pub mod sftp;
pub mod shell;
pub mod ssh;
pub mod state;
pub mod stats;
pub mod store;
pub mod sudo;
pub mod systemd;
pub mod terminal;
pub mod text;
pub mod users;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};
use tauri_specta::{collect_commands, collect_events, Builder, ErrorHandlingMode};

use state::AppState;

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .error_handling(ErrorHandlingMode::Throw)
        .dangerously_cast_bigints_to_number()
        .disable_serde_phases()
        .commands(collect_commands![
            connection::connect,
            connection::disconnect,
            connection::session_info,
            connection::reconnect_now,
            connection::sudo_status,
            connection::sudo_authenticate,
            connection::sudo_forget,
            connection::known_hosts_list,
            connection::known_host_trust,
            connection::known_host_forget,
            local::profiles_list,
            local::profile_save,
            local::profile_delete,
            local::profile_set_default,
            local::profile_resolve_key,
            local::default_key_dir,
            local::profile_data_get,
            local::profile_data_set,
            local::settings_get,
            local::settings_set,
            local::default_download_dir,
            local::startup_notices,
            local::secret_set,
            local::secret_exists,
            local::secret_clear,
            local::save_text_file,
            local::title_bar_color,
            jobs::job_cancel,
            deps::deps_check,
            deps::deps_install,
            stats::stats_basic,
            stats::stats_extended,
            stats::process_list,
            stats::process_signal,
            stats::process_renice,
            terminal::terminal_open,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_close,
            terminal::terminal_cwd,
            terminal::terminal_open_external,
            files::files_list,
            files::files_home,
            files::files_complete,
            files::files_read,
            files::files_write,
            files::files_create,
            files::files_rename,
            files::files_transfer,
            files::files_duplicate,
            files::files_delete,
            files::files_chmod,
            files::files_chown,
            files::files_properties,
            files::files_sizes,
            files::files_search,
            files::files_compress,
            files::files_extract,
            sftp::transfer::transfer_enqueue,
            sftp::transfer::transfer_conflicts,
            sftp::transfer::transfer_list,
            sftp::transfer::transfer_cancel,
            sftp::transfer::transfer_retry_failed,
            sftp::transfer::transfer_clear_completed,
            runbooks::runbook_run,
            systemd::services_list,
            systemd::unit_action,
            systemd::unit_status,
            systemd::unit_logs,
            systemd::unit_logs_follow,
            systemd::unit_file,
            systemd::unit_file_save,
            systemd::service_create,
            systemd::timer_create,
            systemd::unit_delete,
            systemd::timers_list,
            systemd::timer_run_now,
            systemd::timer_inspect,
            cron::cron_list,
            cron::cron_root,
            cron::cron_system_files,
            cron::cron_add,
            cron::cron_update,
            cron::cron_set_enabled,
            cron::cron_delete,
            disks::disks_usage,
            disks::disks_devices,
            disks::disk_mount,
            disks::disk_unmount,
            disks::disk_create_partition,
            disks::disk_expand,
            disks::disk_fsck,
            packages::maintenance_status,
            packages::maintenance_refresh,
            packages::maintenance_upgrade,
            packages::maintenance_auto_updates,
            packages::maintenance_reboot,
            packages::package_search,
            packages::package_change,
            users::accounts_list,
            users::user_create,
            users::user_delete,
            users::user_set_password,
            users::user_set_locked,
            users::user_set_groups,
            users::group_create,
            users::group_delete,
            users::user_keys_get,
            users::user_keys_set,
            env::env_host,
            env::env_container,
            env::env_managed,
            env::env_set,
            env::env_remove,
            docker::docker_overview,
            docker::docker_containers,
            docker::docker_container_action,
            docker::docker_inspect,
            docker::docker_container_detail,
            docker::docker_rename,
            docker::docker_set_restart_policy,
            docker::docker_network_connect,
            docker::docker_recreate,
            docker::docker_logs_follow,
            docker::docker_events_follow,
            docker::docker_exec,
            docker::docker_images,
            docker::docker_image_remove,
            docker::docker_image_pull,
            docker::docker_prune,
            docker::docker_networks,
            docker::docker_network_create,
            docker::docker_network_remove,
            docker::docker_volumes,
            docker::docker_volume_remove,
            docker::docker_stats,
            docker::compose::compose_list,
            docker::compose::compose_action,
            docker::compose::compose_logs,
            docker::compose::compose_validate,
            docker::compose::compose_forget,
            docker::compose::compose_create,
            network::network_listening,
            network::network_connections,
            network::network_interfaces,
            network::netdiag_run,
            network::ip_info,
            firewall::firewall_detect,
            firewall::ufw_status,
            firewall::ufw_set_enabled,
            firewall::ufw_add_rule,
            firewall::ufw_delete_rule,
            firewall::iptables_list,
            firewall::iptables_add_rule,
            firewall::iptables_delete_rule,
            firewall::iptables_set_policy,
            firewall::iptables_persist,
            crowdsec::crowdsec_status,
            crowdsec::crowdsec_decisions,
            crowdsec::crowdsec_ban,
            crowdsec::crowdsec_unban,
            crowdsec::crowdsec_alerts,
            crowdsec::crowdsec_alert_detail,
            crowdsec::crowdsec_bouncers,
            crowdsec::crowdsec_bouncer_add,
            crowdsec::crowdsec_bouncer_delete,
            crowdsec::crowdsec_bouncers_prune,
            crowdsec::crowdsec_metrics,
            crowdsec::crowdsec_source_tail,
            crowdsec::crowdsec_hub,
            crowdsec::crowdsec_hub_action,
            crowdsec::crowdsec_hub_update,
            crowdsec::crowdsec_whitelists,
            crowdsec::crowdsec_whitelist_save,
            crowdsec::crowdsec_allowlist_edit,
            nginx::nginx_hosts,
            nginx::nginx_host_save,
            nginx::nginx_host_delete,
            nginx::nginx_certificates,
            nginx::nginx_cert_issue,
            nginx::nginx_cert_action,
            nginx::nginx_files,
            nginx::nginx_file_read,
            nginx::nginx_file_write,
            nginx::nginx_control,
            logs::logs_sources,
            logs::logs_follow,
            logs::sessions_list,
            logs::sessions_failed,
            logs::session_kick,
            logs::log_analyze,
            db::db_profiles,
            db::db_profile_save,
            db::db_profile_delete,
            db::db_detect,
            db::db_connect,
            db::db_disconnect,
            db::db_connected,
            db::db_databases,
            db::db_schemas,
            db::db_tables,
            db::db_table_structure,
            db::db_table_data,
            db::db_row_insert,
            db::db_row_update,
            db::db_rows_delete,
            db::db_export_table,
            db::db_query,
            restic::restic_repos,
            restic::restic_repo_save,
            restic::restic_repo_delete,
            restic::restic_rclone_remotes,
            restic::restic_status,
            restic::restic_init,
            restic::restic_backup,
            restic::restic_maintenance,
            restic::restic_stats,
            restic::restic_snapshots,
            restic::restic_restore,
            restic::restic_forget,
            restic::restic_forget_policy,
            restic::restic_ls,
            restic::restic_find,
            restic::restic_preview,
            restic::restic_download,
            backups::backup_templates,
            backups::backup_template_save,
            backups::backup_template_delete,
            backups::backup_set_paused,
            backups::backup_run,
            backups::backup_schedule_info,
            pangolin::pangolin_request,
            pangolin::pangolin_status,
            pangolin::pangolin_configure,
            pangolin::pangolin_clear,
        ])
        .events(collect_events![
            connection::ConnectionStatus,
            jobs::JobStarted,
            jobs::JobOutput,
            jobs::JobDone,
            terminal::TerminalData,
            terminal::TerminalExit,
            sftp::transfer::TransferUpdate,
            sftp::transfer::TransferBatchDone,
        ])
        .typ::<error::ErrorCode>()
}

/// Write the TypeScript bindings. Run by `cargo test export_bindings` and on
/// every debug start, so the frontend types always match the backend.
pub fn export_bindings() {
    // Walking the full type graph recurses deeply; give it its own roomy stack.
    let export = || {
        specta_builder()
            .export(
                specta_typescript::Typescript::default().header("// Generated by tauri-specta. Do not edit.\n/* eslint-disable */"),
                concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/ipc/bindings.ts"),
            )
            .expect("failed to export TypeScript bindings");
    };
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(export)
        .expect("failed to start binding export")
        .join()
        .expect("binding export panicked");
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) {
            let _ = window.hide();
        } else {
            show_main_window(app);
        }
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Jarvis Server Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn run() {
    // Some GPU/driver combinations show a blank WebKitGTK window with the
    // DMABUF renderer; the SHM path is slower but always works.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DMABUF_RENDERER_FORCE_SHM").is_none() {
        std::env::set_var("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1");
    }

    let _ = rustls::crypto::ring::default_provider().install_default();

    let builder = specta_builder();
    #[cfg(debug_assertions)]
    export_bindings();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            // JARVIS_CONFIG_DIR points the app at another data directory, which
            // keeps development and test runs away from the real profiles.
            let config_dir = match std::env::var_os("JARVIS_CONFIG_DIR") {
                Some(dir) => std::path::PathBuf::from(dir),
                None => app.path().app_config_dir()?,
            };
            std::fs::create_dir_all(&config_dir)?;
            app.manage(AppState::new(config_dir));
            build_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app (and its sessions) alive in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Jarvis Server Manager");
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_bindings() {
        super::export_bindings();
    }
}
