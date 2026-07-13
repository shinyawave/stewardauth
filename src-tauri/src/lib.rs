// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

mod settings;
mod steam;
mod tray;
mod vault;
mod watch;
pub mod error;
pub mod commands;

use commands::AppState;
use tauri::Manager;
use tokio::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .manage(Mutex::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::get_code,
            commands::import_paths,
            commands::fetch_confirmations,
            commands::respond_confirmation,
            commands::list_login_approvals,
            commands::respond_login_approval,
            commands::login,
            commands::unlock_vault,
            commands::set_encryption,
            commands::remove_account,
            commands::set_auto_confirm,
            commands::set_poll_interval,
            commands::get_poll_interval,
            commands::list_groups,
            commands::create_group,
            commands::delete_group,
            commands::add_to_group,
            commands::remove_from_group,
            commands::export_mafile,
            commands::export_mafiles,
            commands::get_settings,
            commands::set_settings,
            commands::quit_app,
            commands::rescan_mafiles,
            commands::rename_mafiles,
            commands::mafiles_dir,
            commands::list_proxies,
            commands::add_proxies,
            commands::delete_proxy,
            commands::set_proxy_favorite,
            commands::set_default_proxy,
            commands::get_default_proxy,
            commands::assign_proxy,
            commands::bulk_assign_proxy,
            commands::distribute_proxies,
            commands::assign_proxies_by_text,
            commands::unpin_proxy,
            commands::check_proxy,
            commands::get_data_dir,
            commands::set_data_dir,
            commands::link_begin_login,
            commands::link_submit_email_guard,
            commands::link_start,
            commands::link_set_phone,
            commands::link_await_phone_email,
            commands::link_send_sms,
            commands::link_finalize,
            commands::link_cancel,
            commands::link_save_revocation,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.app_handle().state::<Mutex<AppState>>();
                let minimize = state.blocking_lock().minimize_to_tray;
                if minimize {
                    // Hide to tray instead of quitting.
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            // Self-updater (desktop only). Endpoints + pubkey come from
            // tauri.conf.json > plugins.updater.
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            // The FIXED OS app-data dir holds only the bootstrap pointer
            // (location.json). The real data dir is the pointer target, or the
            // fixed dir itself when no pointer is set.
            let fixed_dir = app
                .path()
                .app_data_dir()
                .expect("app data directory should be available");
            // Ensure the fixed dir exists so the pointer can live there.
            std::fs::create_dir_all(&fixed_dir)
                .expect("should be able to create app data directory");
            // One-time migration from the pre-rename bundle id
            // (com.macsda.authenticator). If the previous install's fixed dir
            // still holds the bootstrap pointer or inline data, bring it into the
            // new fixed dir so no accounts are lost by the rename. Best-effort:
            // a failure here must never block startup (worst case: re-import).
            if let Some(legacy_dir) = fixed_dir
                .parent()
                .map(|p| p.join(vault::data_location::LEGACY_BUNDLE_DIR))
            {
                if legacy_dir != fixed_dir {
                    if let Err(e) =
                        vault::data_location::migrate_from_old_bundle(&fixed_dir, &legacy_dir)
                    {
                        eprintln!("bundle-rename migration: {e}");
                    }
                }
            }
            // Resolve the effective data directory and ensure it exists.
            let data_dir = vault::data_location::resolve(&fixed_dir);
            std::fs::create_dir_all(&data_dir)
                .expect("should be able to create data directory");
            // Store the resolved data dir; all commands route through this.
            let state = app.state::<Mutex<AppState>>();
            let mut guard = state.blocking_lock();
            guard.app_data_dir = Some(data_dir.clone());
            match watch::start(app.handle().clone(), &data_dir) {
                Ok(w) => guard.watcher = Some(w),
                Err(e) => eprintln!("watch: failed to start: {e}"),
            }
            drop(guard);

            // Seed the minimize-to-tray mirror from persisted settings.
            {
                let state = app.state::<Mutex<AppState>>();
                let mut guard = state.blocking_lock();
                if let Some(dir) = guard.app_data_dir.clone() {
                    guard.minimize_to_tray = settings::load(&dir).minimize_to_tray;
                }
            }

            // Spawn a one-shot background task to sync Steam server time.
            // On success, stores the offset in AppState.time_sync.
            // On failure, leaves offset at 0 (best-effort).
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use std::time::{SystemTime, UNIX_EPOCH};
                let local_unix = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                // Build a one-off client for this request (AppState.client is
                // behind the Mutex and we must not hold the guard across await).
                let client = reqwest::Client::new();
                match steam::time::fetch_offset(&client, local_unix).await {
                    Ok(offset) => {
                        let state = app_handle.state::<Mutex<AppState>>();
                        let mut guard = state.lock().await;
                        guard.time_sync.offset_secs = offset;
                    }
                    Err(_) => {
                        // Best-effort: leave offset_secs at 0.
                        eprintln!("[stewardauth] Steam time-sync failed; using local clock");
                    }
                }
            });

            // Start the background auto-confirm engine (polls Steam and accepts
            // enabled market/trade confirmations per account). Conservative by
            // design — see steam::auto_confirm for the rate-limit strategy.
            steam::auto_confirm::spawn(app.handle().clone());

            // Build the menu-bar tray icon (Show / Quit).
            tray::setup_tray(app.handle())?;

            // On macOS: run as a regular app so StewardAuth shows a Dock icon
            // (in addition to the menu-bar tray). Clicking the Dock icon while the
            // window is hidden re-shows it — handled by the Reopen event below.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS: clicking the Dock icon (or otherwise re-activating the app)
            // when no window is visible should bring the main window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (app, event);
            }
        });
}
