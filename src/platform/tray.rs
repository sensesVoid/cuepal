use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ToggleVisible,
    ToggleClickThrough,
    Quit,
}

/// Build the tray icon with its menu. Must be called on the main thread
/// before `eframe::run_native` so winit's message pump drives the events.
pub fn create_tray() -> Result<TrayIcon, String> {
    let menu = Menu::new();
    menu.append(&MenuItem::with_id(
        "toggle_visible",
        "Show / Hide overlay",
        true,
        None,
    ))
    .map_err(|e| e.to_string())?;
    menu.append(&MenuItem::with_id(
        "toggle_click_through",
        "Toggle click-through",
        true,
        None,
    ))
    .map_err(|e| e.to_string())?;
    menu.append(&MenuItem::with_id("quit", "Quit", true, None))
        .map_err(|e| e.to_string())?;

    // 16x16 solid green icon, generated in code (no asset files in M0).
    let rgba: Vec<u8> = (0..16 * 16).flat_map(|_| [76u8, 175, 80, 255]).collect();
    let icon = Icon::from_rgba(rgba, 16, 16).map_err(|e| e.to_string())?;

    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("CuePal (spike)")
        .build()
        .map_err(|e| e.to_string())
}

/// Map a tray menu id to a command. Returns None for unknown ids.
pub fn command_for(id: &MenuId) -> Option<TrayCommand> {
    match id.0.as_str() {
        "toggle_visible" => Some(TrayCommand::ToggleVisible),
        "toggle_click_through" => Some(TrayCommand::ToggleClickThrough),
        "quit" => Some(TrayCommand::Quit),
        _ => None,
    }
}

/// Poll the global menu-event channel. Call from `App::ui`.
pub fn poll_commands() -> Vec<TrayCommand> {
    let mut out = Vec::new();
    for ev in MenuEvent::receiver().try_iter() {
        if let Some(c) = command_for(&ev.id) {
            out.push(c);
        }
    }
    out
}
