//! Window workspace layout (SHL-23): the app window and the X Chrome window side by side, remembered across
//! launches. All coordinates are logical pixels in one shared screen space. Pure planning lives here so it is
//! testable; moving the real windows happens in `commands`.

use crate::browser::display::DisplayServer;
use crate::browser::WindowBounds;
use crate::error::AppResult;
use crate::storage::repo;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub const LAYOUT_KEY: &str = "window_layout";
const GAP: u32 = 8;
const MIN_VISIBLE: i64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowLayout {
    pub app: WindowBounds,
    pub browser: WindowBounds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutPlan {
    Apply(WindowLayout),
    /// Placement is not possible here (Wayland); the optional hint is shown to the user instead.
    Skip { hint: Option<String> },
}

/// App on the left, X window on the right, both filling the work area's height.
pub fn default_layout(a: &WorkArea) -> WindowLayout {
    let browser_w = (a.width * 2 / 5).clamp(420, 560).min(a.width / 2);
    let app_w = a.width.saturating_sub(browser_w + GAP);
    WindowLayout {
        app: WindowBounds { left: a.x, top: a.y, width: app_w, height: a.height },
        browser: WindowBounds { left: a.x + app_w as i32 + GAP as i32, top: a.y, width: browser_w, height: a.height },
    }
}

/// `set_size` on a Tauri window sets the content area, while layouts are outer frames (title bar included).
pub fn inner_size(outer: &WindowBounds, frame_w: u32, frame_h: u32) -> (u32, u32) {
    (outer.width.saturating_sub(frame_w), outer.height.saturating_sub(frame_h))
}

fn overlap(b: &WindowBounds, a: &WorkArea) -> (i64, i64) {
    let w = (i64::from(b.left) + i64::from(b.width)).min(i64::from(a.x) + i64::from(a.width)) - i64::from(b.left).max(i64::from(a.x));
    let h = (i64::from(b.top) + i64::from(b.height)).min(i64::from(a.y) + i64::from(a.height)) - i64::from(b.top).max(i64::from(a.y));
    (w, h)
}

/// A saved rectangle is only trusted if enough of it is still on the current screen (monitor unplugged, resolution changed).
fn on_screen(b: &WindowBounds, a: &WorkArea) -> bool {
    let (w, h) = overlap(b, a);
    b.width > 0 && b.height > 0 && w >= MIN_VISIBLE.min(i64::from(b.width)) && h >= MIN_VISIBLE.min(i64::from(b.height))
}

pub fn resolve(saved: Option<WindowLayout>, area: &WorkArea) -> WindowLayout {
    match saved {
        Some(l) if on_screen(&l.app, area) && on_screen(&l.browser, area) => l,
        _ => default_layout(area),
    }
}

pub fn plan(display: DisplayServer, saved: Option<WindowLayout>, area: &WorkArea) -> LayoutPlan {
    if display.supports_window_placement() {
        LayoutPlan::Apply(resolve(saved, area))
    } else {
        LayoutPlan::Skip { hint: display.placement_hint().map(str::to_string) }
    }
}

pub fn load(conn: &Connection) -> AppResult<Option<WindowLayout>> {
    Ok(repo::get_setting(conn, LAYOUT_KEY)?.and_then(|raw| serde_json::from_str(&raw).ok()))
}

pub fn save(conn: &Connection, layout: &WindowLayout) -> AppResult<()> {
    repo::set_setting(conn, LAYOUT_KEY, &serde_json::to_string(layout).expect("layout serializes"))
}

pub fn clear(conn: &Connection) -> AppResult<()> {
    repo::delete_setting(conn, LAYOUT_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::mock::MockDriver;
    use crate::browser::service::BrowserService;
    use crate::browser::{BrowserDriver, ChromeLaunchConfig};

    const SCREEN: WorkArea = WorkArea { x: 0, y: 25, width: 1920, height: 1055 };

    fn mem_db() -> Connection {
        let dir = tempfile::tempdir().unwrap();
        let (db, _) = crate::storage::open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        db
    }

    #[test]
    fn inner_size_removes_the_title_bar_so_both_windows_share_top_and_bottom() {
        let l = default_layout(&SCREEN);
        assert_eq!(inner_size(&l.app, 0, 28), (l.app.width, l.app.height - 28));
        assert_eq!(inner_size(&l.app, 0, 5000), (l.app.width, 0));
    }

    #[test]
    fn default_puts_app_left_and_x_right_without_overlap() {
        let l = default_layout(&SCREEN);
        assert_eq!(l.browser.width, 560);
        assert!(l.app.left + l.app.width as i32 <= l.browser.left);
        assert_eq!(l.browser.left + l.browser.width as i32, 1920, "X window ends at the right edge");
        assert_eq!((l.app.top, l.app.height), (25, 1055));
        assert_eq!((l.browser.top, l.browser.height), (25, 1055));
    }

    #[test]
    fn narrow_screens_shrink_the_x_window_but_keep_both_visible() {
        let l = default_layout(&WorkArea { x: 0, y: 0, width: 1280, height: 720 });
        assert_eq!(l.browser.width, 512);
        assert!(l.app.width > 0 && l.app.left + l.app.width as i32 <= l.browser.left);
    }

    #[test]
    fn saved_layout_is_restored_when_on_screen() {
        let saved = WindowLayout {
            app: WindowBounds { left: 10, top: 40, width: 1000, height: 800 },
            browser: WindowBounds { left: 1050, top: 40, width: 500, height: 800 },
        };
        assert_eq!(resolve(Some(saved.clone()), &SCREEN), saved);
    }

    #[test]
    fn off_screen_saved_layout_falls_back_to_default() {
        let gone = WindowLayout {
            app: WindowBounds { left: 3000, top: 0, width: 900, height: 700 },
            browser: WindowBounds { left: 3900, top: 0, width: 500, height: 700 },
        };
        assert_eq!(resolve(Some(gone), &SCREEN), default_layout(&SCREEN));
        assert_eq!(resolve(None, &SCREEN), default_layout(&SCREEN));
    }

    #[test]
    fn wayland_plans_a_skip_with_a_hint_and_x11_applies() {
        match plan(DisplayServer::Wayland, None, &SCREEN) {
            LayoutPlan::Skip { hint } => assert!(hint.unwrap().contains("Wayland")),
            other => panic!("expected skip, got {other:?}"),
        }
        assert!(matches!(plan(DisplayServer::X11, None, &SCREEN), LayoutPlan::Apply(_)));
        assert!(matches!(plan(DisplayServer::Macos, None, &SCREEN), LayoutPlan::Apply(_)));
    }

    #[test]
    fn layout_survives_a_restart_through_settings() {
        let db = mem_db();
        assert!(load(&db).unwrap().is_none());
        let l = default_layout(&SCREEN);
        save(&db, &l).unwrap();
        assert_eq!(load(&db).unwrap(), Some(l));
        repo::set_setting(&db, LAYOUT_KEY, "{not json").unwrap();
        assert!(load(&db).unwrap().is_none(), "corrupt value is ignored, not fatal");
        clear(&db).unwrap();
        assert!(load(&db).unwrap().is_none());
    }

    #[tokio::test]
    async fn restored_layout_reaches_the_browser_through_the_driver() {
        let mock = MockDriver::new();
        let svc = BrowserService::new(Box::new(mock.clone()));
        let cfg = ChromeLaunchConfig { binary_path: "/x".into(), profile_dir: "/p".into(), start_url: "u".into(), width: 1, height: 1, invisible: false };
        assert!(svc.set_bounds(default_layout(&SCREEN).browser).await.is_err(), "no window yet");
        svc.launch(cfg).await.unwrap();
        let LayoutPlan::Apply(l) = plan(DisplayServer::X11, None, &SCREEN) else { panic!() };
        svc.set_bounds(l.browser.clone()).await.unwrap();
        assert_eq!(mock.get_bounds().await.unwrap(), l.browser);
        assert_eq!(svc.get_bounds().await.unwrap(), l.browser);
    }
}
