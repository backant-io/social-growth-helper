use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

pub const MENU_REPLAY_TOUR: &str = "help_replay_tour";
pub const MENU_READ_RULES: &str = "help_read_rules";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let name = "Social Growth Helper";
    let app_menu = SubmenuBuilder::new(app, name)
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view = SubmenuBuilder::new(app, "View").fullscreen().build()?;
    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().close_window().build()?;
    let help = SubmenuBuilder::new(app, "Help")
        .item(&MenuItemBuilder::with_id(MENU_REPLAY_TOUR, "Replay Intro Tour").build(app)?)
        .item(&MenuItemBuilder::with_id(MENU_READ_RULES, "Read Rules of Engagement").build(app)?)
        .build()?;
    MenuBuilder::new(app).items(&[&app_menu, &edit, &view, &window, &help]).build()
}

/// Maps a menu id to the event the frontend listens for.
pub fn handle_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let action = match id {
        MENU_REPLAY_TOUR => "replay_tour",
        MENU_READ_RULES => "read_rules",
        _ => return,
    };
    let _ = app.emit("menu-action", action);
}
