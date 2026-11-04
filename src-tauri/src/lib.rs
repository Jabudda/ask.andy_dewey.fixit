pub mod command_center;
pub mod commands;
pub mod db;
pub mod documents;
pub mod eligibility;
pub mod error;
pub mod export;
pub mod models;
pub mod parse;

use commands::AppState;
use std::sync::Mutex;
use tauri::Manager;

pub const DB_FILE: &str = "next_chapter.sqlite";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = db::open(&data_dir.join(DB_FILE))?;
            // Refresh expiry and rule checks once per launch.
            if let Ok(p) = db::get_preferences(&conn) {
                let _ = db::reevaluate_all(&conn, &p);
            }
            db::log(&conn, "info", "app", &format!("Started v{}", env!("CARGO_PKG_VERSION")));
            app.manage(AppState { db: Mutex::new(conn), data_dir });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_preferences,
            commands::save_preferences,
            commands::set_milestone,
            commands::get_profile,
            commands::save_profile_basics,
            commands::save_experience,
            commands::save_skill,
            commands::save_certification,
            commands::save_accomplishment,
            commands::delete_profile_item,
            commands::import_document,
            commands::delete_document,
            commands::import_command_center,
            commands::open_document,
            commands::parse_pasted_job,
            commands::fetch_job_from_url,
            commands::save_job,
            commands::update_job,
            commands::list_jobs,
            commands::get_job_detail,
            commands::delete_job,
            commands::change_status,
            commands::update_notes,
            commands::save_contact,
            commands::save_interview,
            commands::save_reminder,
            commands::delete_job_item,
            commands::dashboard_stats,
            commands::export_jobs,
            commands::backup_database,
            commands::restore_database,
            commands::list_logs,
            commands::load_sample_data,
            commands::remove_sample_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Project Next Chapter");
}
