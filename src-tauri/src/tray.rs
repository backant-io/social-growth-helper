//! Menu-bar / tray entry (SHL-08): open the app, bring X to the front, pause or resume everything.

use crate::phase3::Phase3;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main-tray";
const ID_OPEN: &str = "tray_open";
const ID_X: &str = "tray_x";
const ID_KILL: &str = "tray_kill";
const ID_QUIT: &str = "tray_quit";

fn menu(app: &AppHandle, engaged: bool) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id(ID_OPEN, "Open Social Growth Helper").build(app)?)
        .item(&MenuItemBuilder::with_id(ID_X, "Show X window").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(ID_KILL, if engaged { "Resume (kill switch is on)" } else { "Pause everything" }).build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(ID_QUIT, "Quit").build(app)?)
        .build()
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let engaged = app.state::<Phase3>().kill.is_engaged();
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu(app, engaged)?)
        .tooltip(tooltip(engaged))
        .on_menu_event(|app, event| match event.id().as_ref() {
            ID_OPEN => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            ID_X => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = handle.state::<crate::state::AppState>().browser.bring_to_front().await;
                });
            }
            ID_KILL => {
                let engaged = app.state::<Phase3>().kill.is_engaged();
                let _ = crate::phase3::set_kill(app, !engaged);
            }
            ID_QUIT => crate::commands::request_quit(app),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    b.build(app)?;
    Ok(())
}

fn tooltip(engaged: bool) -> &'static str {
    if engaged {
        "Social Growth Helper: PAUSED"
    } else {
        "Social Growth Helper"
    }
}

pub fn refresh(app: &AppHandle, engaged: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(m) = menu(app, engaged) {
            let _ = tray.set_menu(Some(m));
        }
        let _ = tray.set_tooltip(Some(tooltip(engaged)));
    }
}
