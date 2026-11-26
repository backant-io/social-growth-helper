pub mod browser;
mod commands;
pub mod activity;
pub mod completion;
pub mod control;
pub mod drafts;
pub mod error;
pub mod guard;
pub mod lifecycle;
pub mod llm;
mod menu;
pub mod onboarding;
pub mod phase3;
pub mod publish;
pub mod scout;
pub mod secrets;
pub mod selectors;
pub mod state;
pub mod storage;
pub mod style;
mod tray;
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
        .plugin(tauri_plugin_notification::init())
        .menu(|handle| menu::build(handle))
        .on_menu_event(|app, event| menu::handle_event(app, event.id().as_ref()))
        .setup(|app| {
            let state = init_state()?;
            let profile_dir = state.paths.browser_profile_dir.clone();
            let kill_engaged = {
                let db = state.db.lock().unwrap();
                let _ = scout::repo::close_orphans(&db);
                let _ = publish::repo::recover_interrupted(&db);
                phase3::load_kill_state(&db)
            };
            phase3::purge_media(&state.paths.data_dir, phase3::retention_days(&state.db.lock().unwrap()));
            app.manage(state);
            app.manage(phase3::Phase3::new(kill_engaged));
            publish::commands::spawn_worker(app.handle());
            if let Err(e) = tray::install(app.handle()) {
                log::warn!("tray unavailable: {e}");
            }
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
            commands::llm_list_providers,
            commands::llm_detect_claude_cli,
            commands::llm_connect_claude_cli,
            commands::llm_connect_grok_web,
            commands::llm_save_api_key_provider,
            commands::llm_xai_oauth_start,
            commands::llm_xai_oauth_poll,
            commands::llm_health_check_provider,
            commands::llm_refresh_provider_models,
            commands::llm_add_custom_model,
            commands::llm_disconnect_provider,
            commands::llm_list_task_routes,
            commands::llm_save_task_route,
            commands::llm_usage_dashboard,
            commands::llm_list_prompts,
            commands::llm_save_prompt_override,
            commands::llm_reset_prompt,
            commands::style_get_active_guide,
            commands::style_list_guide_versions,
            commands::style_save_guide_version,
            commands::style_activate_guide_version,
            commands::style_list_examples,
            commands::style_search_examples,
            commands::style_upsert_example,
            commands::style_delete_example,
            commands::style_parse_pasted_examples,
            commands::style_import_examples_from_browser,
            commands::style_run_bench,
            phase3::get_kill_switch,
            phase3::set_kill_switch,
            phase3::get_notifications,
            phase3::set_notifications,
            phase3::completion_status,
            phase3::retention_get,
            phase3::retention_set,
            phase3::watchlist_get,
            phase3::watchlist_set,
            phase3::activity_list,
            scout::commands::scout_status,
            scout::commands::scout_start,
            scout::commands::scout_stop,
            scout::commands::scout_pause,
            scout::commands::scout_queue,
            scout::commands::scout_dismiss,
            scout::commands::scout_set_threshold,
            drafts::commands::draft_open,
            drafts::commands::draft_get,
            drafts::commands::draft_save_edit,
            drafts::commands::draft_select_variant,
            drafts::commands::draft_set_actions,
            drafts::commands::draft_discard,
            drafts::commands::draft_check,
            drafts::commands::draft_generate,
            publish::commands::publish_draft,
            publish::commands::engage_action,
            publish::commands::get_action_state,
            publish::commands::get_pacing,
            publish::commands::set_pacing,
            publish::commands::pacing_usage,
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
