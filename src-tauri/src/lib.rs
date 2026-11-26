pub mod browser;
mod commands;
pub mod error;
pub mod lifecycle;
mod menu;
pub mod onboarding;
pub mod secrets;
pub mod state;
pub mod storage;
pub mod util;

use state::{AppPaths, AppState};
use std::sync::atomic::Ordering;
use tauri::{Manager, RunEvent};

fn init_state() -> Result<AppState, Box<dyn std::error::Error>> {
    let paths = AppPaths::resolve();
    std::fs::create_dir_all(&paths.data_dir)?;
    let (db, report) = storage::open(&paths.db_path, &paths.backup_dir)?;
    if !report.applied.is_empty() {
        log::info!("migrated database v{} -> v{}", report.from_version, report.to_version);
    }
    let profile_id = storage::repo::ensure_default_profile(&db, &paths.browser_profile_dir.display().to_string())?;
    Ok(AppState::new(paths, db, profile_id))
}

pub fn run() {
    let app = tauri::Builder::default()
        .menu(|handle| menu::build(handle))
        .on_menu_event(|app, event| menu::handle_event(app, event.id().as_ref()))
        .setup(|app| {
            let state = init_state()?;
            let profile_dir = state.paths.browser_profile_dir.clone();
            app.manage(state);
            // A Chrome left running by a crashed or force-killed previous session is closed before anything else.
            tauri::async_runtime::spawn(async move {
                browser::launcher::reclaim_stale_chrome(&profile_dir).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_onboarding_status,
            commands::get_rules_notice,
            commands::acknowledge_rules,
            commands::complete_tour,
            commands::get_preferences,
            commands::save_preferences,
            commands::secrets_status,
            commands::browser_info,
            commands::set_browser_path,
            commands::browser_launch_x,
            commands::browser_close,
            commands::browser_current_url,
            commands::browser_read_visible_tweets,
            commands::active_jobs,
            commands::quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Social Growth Helper");

    app.run(|handle, event| match event {
        RunEvent::ExitRequested { api, .. } => {
            if !handle.state::<AppState>().quit_approved.load(Ordering::SeqCst) {
                api.prevent_exit();
                commands::request_quit(handle);
            }
        }
        RunEvent::Exit => {
            // Backstop for signals and abnormal exits: the driver's Drop kills a still-running Chrome.
            log::info!("exiting");
        }
        _ => {}
    });
}
