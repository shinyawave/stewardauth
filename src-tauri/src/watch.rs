// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Filesystem watcher for the maFiles directory. On a debounced change it reconciles
//! the manifest with disk and, if anything changed, emits `accounts-changed` so the
//! frontend refreshes. All failures are best-effort: logged, never fatal.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

use crate::commands::AppState;

/// Debounce/coalesce window: collapse a burst of fs events into one reconcile.
const DEBOUNCE: Duration = Duration::from_millis(400);

/// Start watching `<data_dir>/maFiles`. Returns the watcher handle; keep it alive
/// (store it in AppState) or watching stops when it drops.
pub fn start(app: tauri::AppHandle, data_dir: &Path) -> notify::Result<RecommendedWatcher> {
    // Ensure the directory exists so the OS watch can attach.
    let mafiles_dir = crate::vault::mafiles::dir(data_dir);
    let _ = crate::vault::mafiles::ensure_dir(data_dir);

    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            // Coalescing happens on the receiver side; just signal "something changed".
            let _ = tx.send(());
        }
    })?;
    watcher.watch(&mafiles_dir, RecursiveMode::NonRecursive)?;

    // Debounce thread: on the first signal, drain the window, then reconcile once.
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            // Coalesce further events within the debounce window.
            while rx.recv_timeout(DEBOUNCE).is_ok() {}
            reconcile_and_emit(&app);
        }
    });

    Ok(watcher)
}

/// Read app_dir + master from state (blocking lock — this runs on a plain thread,
/// not an async context), reconcile, and emit `accounts-changed` if changed.
fn reconcile_and_emit(app: &tauri::AppHandle) {
    let state = app.state::<Mutex<AppState>>();
    let (app_dir, master) = {
        let guard = state.blocking_lock();
        match guard.app_data_dir.clone() {
            Some(dir) => (dir, guard.master.clone()),
            None => return,
        }
    };
    match crate::vault::store::reconcile_folder(&app_dir, master.as_deref()) {
        Ok(true) => {
            let _ = app.emit("accounts-changed", ());
        }
        Ok(false) => {}
        Err(e) => eprintln!("watch: reconcile failed: {e}"),
    }
}
