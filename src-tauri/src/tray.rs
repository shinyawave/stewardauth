// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Menu-bar tray for StewardAuth.
//!
//! Creates a system-tray icon with a two-item menu (Show / Quit).
//! On macOS the app runs as a Regular app (Dock icon visible) and also
//! keeps this menu-bar tray; both can reveal the hidden main window.
//!
//! # Live code label (deferred)
//! A per-30-second timer that reads AppState + Keychain and updates a disabled
//! menu item with the active account's code would require holding an `AppHandle`
//! reference inside a spawned loop plus re-building or mutating the menu item
//! text at runtime.  This is doable but adds non-trivial complexity (the
//! `MenuItem` would need to be stored in an Arc/Mutex alongside the tray so the
//! timer callback can call `menu_item.set_text()`).  It is noted here as a
//! follow-up to keep Task 16 focused and ship-safe.

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};

/// Build and register the tray icon.
///
/// Call this from `setup` immediately after initialising `AppState`.
pub fn setup_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show StewardAuth", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app: &AppHandle<R>, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        });

    // Attach the app icon when available — tray still works without it (the OS
    // will show a placeholder), so we silently skip any failure.
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    let _tray = builder.build(app)?;
    Ok(())
}
