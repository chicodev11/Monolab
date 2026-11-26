//! Tray residency: the main window's X hides Monolab (servers, backups, the web panel and Monolink
//! keep running) and the tray's Quit is the way out. Quitting goes through the screen first, which
//! runs the same checks closing the window used to (running servers, the web panel, unsaved edits).

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Window};
use tauri_plugin_notification::NotificationExt;

/// Sent to the main window when the user asks to quit; the screen answers with `quit_app`.
pub const QUIT_REQUESTED: &str = "quit-requested";
/// Given by "Start with Windows": the app starts in the tray without showing the window.
pub const HIDDEN_ARG: &str = "--hidden";

pub fn show_main(app: &AppHandle) {
    // The window, not its webview: a team view inside it makes it hold more than one.
    if let Some(window) = app.get_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn ask_to_quit(app: &AppHandle) {
    show_main(app);
    let _ = app.emit_to("main", QUIT_REQUESTED, ());
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Monolab")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "quit" => ask_to_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// The main window's X: hide to the tray, or quit (through the screen's checks) when the user chose that.
pub fn on_close(window: &Window) {
    let app = window.app_handle();
    let mut settings = crate::read_app_settings(app);
    if !settings.close_to_tray {
        ask_to_quit(app);
        return;
    }
    let _ = window.hide();
    if !settings.tray_hint_shown {
        settings.tray_hint_shown = true;
        let _ = crate::write_app_settings(app, &settings);
        let _ = app
            .notification()
            .builder()
            .title("Monolab")
            .body("Monolab is still running in the tray. Use Quit there to exit.")
            .show();
    }
}

/// Ends the app. The screen calls it once the quit checks pass; `RunEvent::Exit` cleanup follows.
#[tauri::command]
#[specta::specta]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command(async)]
#[specta::specta]
pub fn autostart_enabled(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|error| error.to_string())
}

#[tauri::command(async)]
#[specta::specta]
pub fn set_autostart(app: AppHandle, on: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if on { launcher.enable() } else { launcher.disable() }.map_err(|error| format!("Could not change Start with Windows: {error}"))
}
