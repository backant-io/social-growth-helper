//! Menu-bar / tray entry (SHL-08): jump to the reply queue with its waiting count, pause or resume Scout,
//! open the app, bring X to the front, pause or resume everything.

use crate::phase3::Phase3;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ID: &str = "main-tray";
const ID_OPEN: &str = "tray_open";
const ID_QUEUE: &str = "tray_queue";
const ID_SCOUT: &str = "tray_scout";
const ID_X: &str = "tray_x";
const ID_KILL: &str = "tray_kill";
const ID_QUIT: &str = "tray_quit";

/// Everything the menu text depends on.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Snapshot {
    pub engaged: bool,
    pub queued: usize,
    /// `None` when no Scout session is running.
    pub scout_paused: Option<bool>,
}

pub fn queue_label(queued: usize) -> String {
    match queued {
        0 => "Reply queue (empty)".into(),
        1 => "Reply queue (1 waiting)".into(),
        n => format!("Reply queue ({n} waiting)"),
    }
}

pub fn scout_label(s: &Snapshot) -> Option<&'static str> {
    match s.scout_paused {
        Some(false) => Some("Pause Scout"),
        Some(true) => Some("Resume Scout"),
        None => None,
    }
}

fn kill_label(engaged: bool) -> &'static str {
    if engaged {
        "Resume (kill switch is on)"
    } else {
        "Pause everything"
    }
}

fn tooltip(s: &Snapshot) -> String {
    let base = if s.engaged { "Social Growth Helper: PAUSED" } else { "Social Growth Helper" };
    if s.queued > 0 {
        format!("{base} ({} waiting)", s.queued)
    } else {
        base.to_string()
    }
}

fn menu(app: &AppHandle, s: &Snapshot) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let mut b = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id(ID_QUEUE, queue_label(s.queued)).build(app)?)
        .item(&MenuItemBuilder::with_id(ID_OPEN, "Open Social Growth Helper").build(app)?)
        .item(&MenuItemBuilder::with_id(ID_X, "Show X window").build(app)?)
        .separator();
    if let Some(label) = scout_label(s) {
        b = b.item(&MenuItemBuilder::with_id(ID_SCOUT, label).build(app)?);
    }
    b.item(&MenuItemBuilder::with_id(ID_KILL, kill_label(s.engaged)).build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(ID_QUIT, "Quit").build(app)?)
        .build()
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn snapshot(app: &AppHandle) -> Snapshot {
    let p3 = app.state::<Phase3>();
    let state = app.state::<crate::state::AppState>();
    let queued = {
        let db = state.db.lock().unwrap();
        let threshold = crate::scout::repo::get_threshold(&db).unwrap_or(0.0);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        crate::scout::repo::queue(&db, &state.profile_id, threshold, now, 500).map(|q| q.len()).unwrap_or(0)
    };
    let scout_paused = p3.scout.lock().unwrap().as_ref().map(|s| s.pause.load(std::sync::atomic::Ordering::SeqCst));
    Snapshot { engaged: p3.kill.is_engaged(), queued, scout_paused }
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let snap = snapshot(app);
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu(app, &snap)?)
        .tooltip(tooltip(&snap))
        .on_menu_event(|app, event| match event.id().as_ref() {
            ID_OPEN => show_main(app),
            ID_QUEUE => {
                show_main(app);
                let _ = app.emit("nav:go", "scout");
            }
            ID_SCOUT => {
                let paused = app.state::<Phase3>().scout.lock().unwrap().as_ref().map(|s| s.pause.load(std::sync::atomic::Ordering::SeqCst));
                if let Some(p) = paused {
                    crate::scout::commands::set_scout_paused(&app.state::<Phase3>(), !p);
                    refresh_all(app);
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

/// Rebuilds the menu and tooltip from current state. Cheap; call after anything that changes the queue,
/// the kill switch or the Scout session.
pub fn refresh_all(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let snap = snapshot(app);
        if let Ok(m) = menu(app, &snap) {
            let _ = tray.set_menu(Some(m));
        }
        let _ = tray.set_tooltip(Some(tooltip(&snap)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_label_counts_and_pluralises() {
        assert_eq!(queue_label(0), "Reply queue (empty)");
        assert_eq!(queue_label(1), "Reply queue (1 waiting)");
        assert_eq!(queue_label(12), "Reply queue (12 waiting)");
    }

    #[test]
    fn scout_item_only_exists_while_a_session_runs_and_flips_with_its_state() {
        let mut s = Snapshot::default();
        assert_eq!(scout_label(&s), None);
        s.scout_paused = Some(false);
        assert_eq!(scout_label(&s), Some("Pause Scout"));
        s.scout_paused = Some(true);
        assert_eq!(scout_label(&s), Some("Resume Scout"));
    }

    #[test]
    fn tooltip_shows_pause_and_waiting_count() {
        assert_eq!(tooltip(&Snapshot::default()), "Social Growth Helper");
        assert_eq!(tooltip(&Snapshot { engaged: true, queued: 3, scout_paused: None }), "Social Growth Helper: PAUSED (3 waiting)");
    }
}
