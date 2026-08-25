//! The application menu, and the two doors a macOS user reaches for first.
//!
//! Tauri's default menu carries About, Services, Hide and Quit. So ⌘, — the
//! shortcut every application on the platform answers with its settings — did
//! nothing, and the menu offered no way in either. The Settings screen existed
//! and was reachable only from the top bar and the tray.
//!
//! The item is inserted into the default menu rather than replacing it.
//! Building a menu by hand means owning File, Edit, View, Window and Help,
//! along with the platform behaviour their predefined items carry — copy,
//! paste, minimise, full screen, the window list — for the sake of one line.

use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Runtime};

/// Matched by the menu event handler. The tray uses this same string for the
/// same destination, so both routes to Settings turn up in one grep.
pub const SETTINGS_ID: &str = "settings";

/// What the interface listens for.
///
/// Navigation happens in React rather than by assigning `window.location`,
/// which is what the tray does: a full document load rebuilds every context
/// and throws away the transcript view's state. That is a steep price for a
/// keystroke somebody may have hit by accident during a recording.
const SETTINGS_EVENT: &str = "request-settings";

/// Build the menu handed to `tauri::Builder::menu`.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;

    let items = menu.items()?;
    let Some(first_submenu) = items.first().and_then(|item| item.as_submenu()) else {
        // Not worth failing a launch over. The application still runs with the
        // default menu; it is missing one entry, and the log says so.
        log::warn!("Default menu has no submenu to extend; Settings item not added");
        return Ok(menu);
    };

    let settings = MenuItemBuilder::with_id(SETTINGS_ID, "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;

    if cfg!(target_os = "macos") {
        // Directly under "About Halvern", where macOS applications put it and
        // where people look without reading.
        first_submenu.insert(&separator, 1)?;
        first_submenu.insert(&settings, 2)?;
    } else {
        // Elsewhere the first submenu is File rather than an application menu.
        // Windows and Linux are inherited targets here and neither is
        // released, so this is a reasonable placement rather than a researched
        // one — the accelerator, which is what people actually use, is right
        // on every platform.
        first_submenu.insert(&settings, 0)?;
        first_submenu.insert(&separator, 1)?;
    }

    Ok(menu)
}

/// Handle a menu selection. Ignores anything that is not ours: the tray has
/// its own handler and its own identifiers.
pub fn handle_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if id != SETTINGS_ID {
        return;
    }

    crate::tray::focus_main_window(app);

    if let Err(e) = app.emit(SETTINGS_EVENT, ()) {
        log::warn!("Could not ask the interface to open Settings: {}", e);
    }
}
