pub mod command_center;
pub mod ai;
pub mod commands;
pub mod connectors;
pub mod discovery;
pub mod docgen;
pub mod db;
pub mod documents;
pub mod eligibility;
pub mod error;
pub mod export;
pub mod generate;
pub mod models;
pub mod parse;
pub mod scoring;
pub mod secrets;
pub mod tailoring;

use commands::AppState;
use std::sync::Mutex;
use tauri::{Emitter, Manager};

pub const DB_FILE: &str = "next_chapter.sqlite";

/// Runs scheduled searches while the app is open. Checks once a minute; the interval lives in
/// settings (0 = off). Uses its own database connection so the UI is never blocked.
fn start_scheduler(handle: tauri::AppHandle, db_path: std::path::PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(90));
        loop {
            let due = db::open(&db_path)
                .and_then(|c| {
                    let d = discovery::due(&c, chrono::Local::now().naive_local())?;
                    if d {
                        discovery::mark_scheduled(&c)?;
                    }
                    Ok(d)
                })
                .unwrap_or(false);
            if due {
                match tauri::async_runtime::block_on(discovery::run_with_path(&db_path, None, "scheduled")) {
                    Ok(summaries) => {
                        let _ = handle.emit("discovery-finished", &summaries);
                    }
                    Err(e) => {
                        if let Ok(c) = db::open(&db_path) {
                            db::log(&c, "error", "schedule", &format!("Scheduled search failed: {e}"));
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    });
}

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
            app.manage(AppState { db: Mutex::new(conn), data_dir: data_dir.clone() });
            start_scheduler(app.handle().clone(), data_dir.join(DB_FILE));
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
            commands::save_education,
            commands::rescore_jobs,
            commands::list_sources,
            commands::save_source,
            commands::delete_source,
            commands::detect_source,
            commands::list_search_runs,
            commands::get_search_settings,
            commands::set_search_interval,
            commands::run_discovery,
            commands::credential_status,
            commands::get_ai_settings,
            commands::save_ai_settings,
            commands::ai_usage_summary,
            commands::prepare_generation,
            commands::generate_document,
            commands::save_document_edit,
            commands::approve_document,
            commands::open_document_pdf,
            commands::save_document_copy,
            commands::reveal_document,
            commands::save_credential,
            commands::delete_credential,
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
