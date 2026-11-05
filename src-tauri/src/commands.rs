//! Tauri IPC surface. Thin wrappers: validation and rules live in db/parse/eligibility.

use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::{db, documents, export, parse};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub data_dir: PathBuf,
}

impl AppState {
    fn conn(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        self.db.lock().map_err(|_| AppError::other("Database is busy after an earlier error; restart the app"))
    }
    fn documents_dir(&self) -> PathBuf {
        self.data_dir.join("documents")
    }
}

type S<'a> = State<'a, AppState>;

#[tauri::command]
pub fn app_info(state: S) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: state.data_dir.display().to_string(),
        database_path: state.data_dir.join(crate::DB_FILE).display().to_string(),
        documents_dir: state.documents_dir().display().to_string(),
    }
}

// ── preferences & settings ──
#[tauri::command]
pub fn get_preferences(state: S) -> AppResult<Preferences> {
    db::get_preferences(&*state.conn()?)
}
#[tauri::command]
pub fn save_preferences(state: S, prefs: Preferences) -> AppResult<usize> {
    db::save_preferences(&*state.conn()?, &prefs)
}
#[tauri::command]
pub fn set_milestone(state: S, date: String) -> AppResult<()> {
    db::set_milestone(&*state.conn()?, &date)
}

// ── profile ──
#[tauri::command]
pub fn get_profile(state: S) -> AppResult<ProfileBundle> {
    db::get_profile(&*state.conn()?)
}
#[tauri::command]
pub fn save_profile_basics(state: S, basics: ProfileBasics) -> AppResult<()> {
    db::save_profile_basics(&*state.conn()?, &basics)
}
#[tauri::command]
pub fn save_experience(state: S, item: Experience) -> AppResult<i64> {
    { let c = state.conn()?; let id = db::save_experience(&c, &item)?; db::score_jobs(&c, None)?; Ok(id) }
}
#[tauri::command]
pub fn save_skill(state: S, item: Skill) -> AppResult<i64> {
    { let c = state.conn()?; let id = db::save_skill(&c, &item)?; db::score_jobs(&c, None)?; Ok(id) }
}
#[tauri::command]
pub fn save_certification(state: S, item: Certification) -> AppResult<i64> {
    { let c = state.conn()?; let id = db::save_certification(&c, &item)?; db::score_jobs(&c, None)?; Ok(id) }
}
#[tauri::command]
pub fn save_accomplishment(state: S, item: Accomplishment) -> AppResult<i64> {
    { let c = state.conn()?; let id = db::save_accomplishment(&c, &item)?; db::score_jobs(&c, None)?; Ok(id) }
}
#[tauri::command]
pub fn delete_profile_item(state: S, section: String, id: i64) -> AppResult<()> {
    let c = state.conn()?;
    db::delete_profile_item(&c, &section, id)?;
    db::score_jobs(&c, None)?;
    Ok(())
}
#[tauri::command]
pub fn save_education(state: S, item: Education) -> AppResult<i64> {
    let c = state.conn()?;
    let id = db::save_education(&c, &item)?;
    db::score_jobs(&c, None)?;
    Ok(id)
}
#[tauri::command]
pub fn rescore_jobs(state: S) -> AppResult<usize> {
    db::score_jobs(&*state.conn()?, None)
}

/// Copies a user-selected file into the app's documents folder and extracts its text.
pub fn import_document_into(c: &Connection, docs_dir: &Path, src: &Path, kind: &str) -> AppResult<DocumentImportResult> {
    const KINDS: [&str; 5] = ["master_resume", "career_document", "certification", "accomplishment", "writing_sample"];
    if !KINDS.contains(&kind) {
        return Err(AppError::invalid("Unknown document type"));
    }
    let meta = std::fs::metadata(src)?;
    if meta.len() > 25 * 1024 * 1024 {
        return Err(AppError::invalid("Files larger than 25 MB are not supported"));
    }
    let (text, warning) = documents::extract_text(src)?;
    let bytes = std::fs::read(src)?;
    let sha = documents::sha256_hex(&bytes);
    let file_name = src.file_name().and_then(|n| n.to_str()).unwrap_or("document").to_string();
    std::fs::create_dir_all(docs_dir)?;
    let dest = docs_dir.join(format!("{}_{}_{}", kind, chrono::Local::now().format("%Y%m%d%H%M%S"), file_name));
    std::fs::copy(src, &dest)?;
    let doc = db::insert_document(c, kind, &file_name, &dest.display().to_string(), &sha, &text, None)?;
    let existing: Vec<String> = db::get_profile(c)?.skills.into_iter().map(|s| s.name.to_lowercase()).collect();
    let suggested_skills = documents::suggest_skills(&text)
        .into_iter()
        .filter(|s| !existing.contains(&s.to_lowercase()))
        .collect();
    db::log(c, "info", "documents", &format!("Imported {kind}: {file_name} ({} characters of text)", text.chars().count()));
    if let Some(w) = &warning {
        db::log(c, "warn", "documents", w);
    }
    Ok(DocumentImportResult { document: doc, suggested_skills, warning })
}

#[tauri::command]
pub fn import_document(state: S, path: String, kind: String) -> AppResult<DocumentImportResult> {
    let c = state.conn()?;
    let r = import_document_into(&c, &state.documents_dir(), Path::new(&path), &kind).inspect_err(|e| {
        db::log(&c, "error", "documents", &format!("Import failed for {path}: {e}"));
    })?;
    if kind == "master_resume" {
        db::score_jobs(&c, None)?;
    }
    Ok(r)
}

#[tauri::command]
pub fn delete_document(state: S, id: i64) -> AppResult<()> {
    let c = state.conn()?;
    let d = db::delete_document(&c, id)?;
    // Only remove files inside our own documents folder.
    let p = PathBuf::from(&d.stored_path);
    if p.starts_with(state.documents_dir()) {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

// ── job import ──
#[tauri::command]
pub fn parse_pasted_job(text: String) -> AppResult<JobDraft> {
    if text.trim().len() < 20 {
        return Err(AppError::invalid("Paste the full job posting text (title, employer and description)"));
    }
    Ok(parse::parse_pasted(&text))
}

#[tauri::command]
pub async fn fetch_job_from_url(state: State<'_, AppState>, url: String) -> AppResult<JobDraft> {
    let parsed = url::Url::parse(url.trim()).map_err(|_| AppError::invalid("That is not a valid web address"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(AppError::invalid("Only http(s) links can be imported"));
    }
    let result = fetch_html(parsed.as_str()).await;
    let c = state.conn()?;
    match result {
        Ok(html) => {
            let draft = match parse::find_jobposting_jsonld(&html) {
                Some(v) => parse::draft_from_jsonld(&v, parsed.as_str()),
                None => parse::draft_from_html_fallback(&html, parsed.as_str()),
            };
            db::log(&c, "info", "fetch", &format!("Fetched {} ({} extraction)", parsed, draft.extraction_method));
            Ok(draft)
        }
        Err(e) => {
            db::log(&c, "error", "fetch", &format!("Could not fetch {parsed}: {e}"));
            Err(e)
        }
    }
}

async fn fetch_html(url: &str) -> AppResult<String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("ProjectNextChapter/", env!("CARGO_PKG_VERSION"), " (personal job tracker; single user-requested page fetch)"))
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .https_only(false)
        .build()
        .map_err(AppError::other)?;
    let resp = client.get(url).send().await.map_err(|e| AppError::other(format!("Network error: {e}")))?;
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 || status.as_u16() == 429 {
        return Err(AppError::other(format!(
            "The site refused automated access (HTTP {status}). Open the posting in your browser and paste its text instead."
        )));
    }
    if !status.is_success() {
        return Err(AppError::other(format!("The site returned HTTP {status}. The posting may have been removed.")));
    }
    let body = resp.text().await.map_err(AppError::other)?;
    Ok(body.chars().take(3_000_000).collect())
}

// ── folder import ──
#[tauri::command]
pub async fn import_command_center(state: State<'_, AppState>, path: String, dry_run: bool) -> AppResult<crate::command_center::ImportReport> {
    let c = state.conn()?;
    let r = crate::command_center::run(&c, &state.documents_dir(), Path::new(&path), dry_run)
        .inspect_err(|e| db::log(&c, "error", "import", &format!("Folder import failed: {e}")))?;
    if !dry_run {
        db::score_jobs(&c, None)?;
    }
    Ok(r)
}

/// Opens a stored document with its default macOS app (only files this app knows about).
#[tauri::command]
pub fn open_document(state: S, id: i64) -> AppResult<()> {
    let d = db::get_document(&*state.conn()?, id)?;
    std::process::Command::new("open").arg(&d.stored_path).spawn().map_err(AppError::other)?;
    Ok(())
}

// ── discovery ──
#[tauri::command]
pub fn list_sources(state: S) -> AppResult<Vec<JobSource>> {
    crate::discovery::list_sources(&*state.conn()?)
}
#[tauri::command]
pub fn save_source(state: S, source: JobSource) -> AppResult<i64> {
    crate::discovery::save_source(&*state.conn()?, &source)
}
#[tauri::command]
pub fn delete_source(state: S, id: i64) -> AppResult<()> {
    crate::discovery::delete_source(&*state.conn()?, id)
}
#[tauri::command]
pub fn detect_source(url: String) -> Option<(String, String)> {
    crate::connectors::detect_source(&url)
}
#[tauri::command]
pub fn list_search_runs(state: S) -> AppResult<Vec<SearchRun>> {
    crate::discovery::list_runs(&*state.conn()?, 60)
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSettings {
    pub interval_hours: i64,
    pub last_scheduled: Option<String>,
}
#[tauri::command]
pub fn get_search_settings(state: S) -> AppResult<SearchSettings> {
    let c = state.conn()?;
    Ok(SearchSettings { interval_hours: crate::discovery::interval_hours(&c)?, last_scheduled: crate::discovery::last_scheduled(&c)? })
}
#[tauri::command]
pub fn set_search_interval(state: S, hours: i64) -> AppResult<()> {
    crate::discovery::set_interval_hours(&*state.conn()?, hours)
}
#[tauri::command]
pub async fn run_discovery(state: State<'_, AppState>, source_id: Option<i64>) -> AppResult<Vec<crate::discovery::RunSummary>> {
    crate::discovery::run_with_path(&state.data_dir.join(crate::DB_FILE), source_id, "manual").await
}

// ── credentials (macOS Keychain; values never returned to the UI) ──
#[tauri::command]
pub fn credential_status(name: String) -> bool {
    crate::secrets::has(&name)
}
#[tauri::command]
pub fn save_credential(state: S, name: String, value: String) -> AppResult<()> {
    crate::secrets::set(&name, &value)?;
    db::log(&*state.conn()?, "info", "credentials", &format!("{name} API key saved to the macOS Keychain"));
    Ok(())
}
#[tauri::command]
pub fn delete_credential(state: S, name: String) -> AppResult<()> {
    crate::secrets::delete(&name)?;
    db::log(&*state.conn()?, "info", "credentials", &format!("{name} API key removed from the macOS Keychain"));
    Ok(())
}

// ── AI writing & generated documents ──
#[tauri::command]
pub fn get_ai_settings(state: S) -> AppResult<crate::ai::AiSettings> {
    crate::ai::get_settings(&*state.conn()?)
}
#[tauri::command]
pub fn save_ai_settings(state: S, settings: crate::ai::AiSettings) -> AppResult<()> {
    crate::ai::save_settings(&*state.conn()?, &settings)
}
#[tauri::command]
pub fn ai_usage_summary(state: S) -> AppResult<crate::ai::UsageSummary> {
    crate::ai::usage_summary(&*state.conn()?)
}
#[tauri::command]
pub fn prepare_generation(state: S, job_id: i64, kind: String, learn_from_previous: bool) -> AppResult<crate::generate::Prepared> {
    crate::generate::prepare(&*state.conn()?, job_id, &kind, learn_from_previous)
}
#[tauri::command]
pub async fn generate_document(state: State<'_, AppState>, job_id: i64, kind: String, learn_from_previous: bool) -> AppResult<crate::generate::Generated> {
    let (prep, body) = {
        let c = state.conn()?;
        let s = crate::ai::get_settings(&c)?;
        if !s.enabled || s.acknowledged_at.is_none() {
            return Err(AppError::invalid("Turn on AI writing in Preferences first"));
        }
        let p = crate::generate::prepare(&c, job_id, &kind, learn_from_previous)?;
        let b = crate::generate::request_body(&p);
        (p, b)
    };
    let key = crate::secrets::get("anthropic")?.ok_or_else(|| AppError::invalid("Add your Anthropic API key in Preferences → AI writing"))?;
    let response = match crate::ai::send(&key, &body).await {
        Ok(v) => v,
        Err(e) => {
            db::log(&*state.conn()?, "error", "ai", &e);
            return Err(AppError::other(e));
        }
    };
    let c = state.conn()?;
    crate::generate::finish(&c, &state.documents_dir(), &prep, &response)
}
#[tauri::command]
pub fn save_document_edit(state: S, id: i64, content: serde_json::Value) -> AppResult<crate::generate::Generated> {
    crate::generate::save_edit(&*state.conn()?, &state.documents_dir(), id, &content)
}
#[tauri::command]
pub fn approve_document(state: S, id: i64, reviewed: bool) -> AppResult<DocumentRec> {
    crate::generate::approve(&*state.conn()?, id, reviewed)
}
/// Saves a copy of a document (Word or PDF) wherever the user chose in the Save dialog.
#[tauri::command]
pub fn save_document_copy(state: S, id: i64, format: String, dest: String) -> AppResult<()> {
    let d = db::get_document(&*state.conn()?, id)?;
    let src = match format.as_str() {
        "pdf" => d.pdf_path.ok_or_else(|| AppError::invalid("This document has no PDF version"))?,
        _ => d.stored_path,
    };
    std::fs::copy(&src, &dest).map_err(AppError::other)?;
    db::log(&*state.conn()?, "info", "documents", &format!("Saved a copy of document #{id} ({format}) to {dest}"));
    Ok(())
}
/// Shows the document's file in Finder.
#[tauri::command]
pub fn reveal_document(state: S, id: i64) -> AppResult<()> {
    let d = db::get_document(&*state.conn()?, id)?;
    let p = d.pdf_path.unwrap_or(d.stored_path);
    std::process::Command::new("open").arg("-R").arg(p).spawn().map_err(AppError::other)?;
    Ok(())
}
#[tauri::command]
pub fn open_document_pdf(state: S, id: i64) -> AppResult<()> {
    let d = db::get_document(&*state.conn()?, id)?;
    let p = d.pdf_path.ok_or_else(|| AppError::invalid("This document has no PDF version"))?;
    std::process::Command::new("open").arg(p).spawn().map_err(AppError::other)?;
    Ok(())
}

// ── jobs ──
#[tauri::command]
pub fn save_job(state: S, draft: JobDraft, force: bool) -> AppResult<SaveJobResult> {
    db::insert_job(&*state.conn()?, &draft, force, false)
}
#[tauri::command]
pub fn update_job(state: S, id: i64, draft: JobDraft) -> AppResult<Job> {
    db::update_job(&*state.conn()?, id, &draft)
}
#[tauri::command]
pub fn list_jobs(state: S) -> AppResult<Vec<Job>> {
    db::list_jobs(&*state.conn()?)
}
#[tauri::command]
pub fn get_job_detail(state: S, id: i64) -> AppResult<JobDetail> {
    db::get_job_detail(&*state.conn()?, id)
}
#[tauri::command]
pub fn delete_job(state: S, id: i64) -> AppResult<()> {
    db::delete_job(&*state.conn()?, id)
}
#[tauri::command]
pub fn change_status(state: S, change: StatusChange) -> AppResult<Job> {
    db::change_status(&*state.conn()?, &change)
}
#[tauri::command]
pub fn update_notes(state: S, id: i64, notes: String) -> AppResult<()> {
    db::update_notes(&*state.conn()?, id, &notes)
}
#[tauri::command]
pub fn save_contact(state: S, item: Contact) -> AppResult<i64> {
    db::save_contact(&*state.conn()?, &item)
}
#[tauri::command]
pub fn save_interview(state: S, item: Interview) -> AppResult<i64> {
    db::save_interview(&*state.conn()?, &item)
}
#[tauri::command]
pub fn save_reminder(state: S, item: Reminder) -> AppResult<i64> {
    db::save_reminder(&*state.conn()?, &item)
}
#[tauri::command]
pub fn delete_job_item(state: S, kind: String, id: i64) -> AppResult<()> {
    db::delete_child(&*state.conn()?, &kind, id)
}

// ── dashboard ──
#[tauri::command]
pub fn dashboard_stats(state: S) -> AppResult<DashboardStats> {
    db::dashboard_stats(&*state.conn()?, db::today())
}

// ── data ──
#[tauri::command]
pub fn export_jobs(state: S, path: String, format: String) -> AppResult<usize> {
    let c = state.conn()?;
    let p = Path::new(&path);
    let n = match format.as_str() {
        "csv" => export::export_csv(&c, p)?,
        "xlsx" => export::export_xlsx(&c, p)?,
        _ => return Err(AppError::invalid("Format must be csv or xlsx")),
    };
    db::log(&c, "info", "export", &format!("Exported {n} jobs to {path}"));
    Ok(n)
}
#[tauri::command]
pub fn backup_database(state: S, path: String) -> AppResult<()> {
    let c = state.conn()?;
    export::backup(&c, Path::new(&path)).inspect_err(|e| db::log(&c, "error", "backup", &e.to_string()))?;
    db::log(&c, "info", "backup", &format!("Backup written to {path}"));
    Ok(())
}
#[tauri::command]
pub fn restore_database(state: S, path: String) -> AppResult<String> {
    let mut c = state.conn()?;
    let safety = export::restore(&mut c, Path::new(&path), &state.data_dir.join("backups"))
        .inspect_err(|e| db::log(&c, "error", "backup", &format!("Restore failed: {e}")))?;
    Ok(safety.display().to_string())
}
#[tauri::command]
pub fn list_logs(state: S, query: String) -> AppResult<Vec<LogEntry>> {
    db::list_logs(&*state.conn()?, &query, 500)
}
#[tauri::command]
pub fn load_sample_data(state: S) -> AppResult<usize> {
    db::load_sample_data(&*state.conn()?)
}
#[tauri::command]
pub fn remove_sample_data(state: S) -> AppResult<usize> {
    db::remove_sample_data(&*state.conn()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_import_copies_and_suggests() {
        let dir = tempfile::tempdir().unwrap();
        let c = db::open_in_memory().unwrap();
        db::save_skill(&c, &Skill { name: "Excel".into(), ..Default::default() }).unwrap();
        let src = dir.path().join("resume.txt");
        std::fs::write(&src, "Smartsheet automation and Excel Office Scripts").unwrap();
        let docs = dir.path().join("documents");
        let r = import_document_into(&c, &docs, &src, "master_resume").unwrap();
        assert!(Path::new(&r.document.stored_path).exists());
        assert!(r.suggested_skills.contains(&"Smartsheet".to_string()));
        assert!(!r.suggested_skills.contains(&"Excel".to_string()), "already-verified skills are not re-suggested");
        assert!(import_document_into(&c, &docs, &src, "bogus").is_err());
        // Skills are suggestions only — nothing was added
        assert_eq!(db::get_profile(&c).unwrap().skills.len(), 1);
    }

    #[test]
    fn imports_bundled_sample_resume() {
        let dir = tempfile::tempdir().unwrap();
        let c = db::open_in_memory().unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sample-data/sample_master_resume.txt");
        let r = import_document_into(&c, dir.path(), &src, "master_resume").unwrap();
        assert!(r.document.extracted_text.contains("SAMPLE DATA"));
        assert!(r.suggested_skills.len() >= 5);
    }
}

// ── Phase 4: review queue & application assistance ──
use crate::application::{self as app, BankAnswer, JobAnswer, Packet, QueueItem};
#[tauri::command]
pub fn review_queue(state: S) -> AppResult<Vec<QueueItem>> {
    app::queue(&*state.conn()?)
}
#[tauri::command]
pub fn application_packet(state: S, job_id: i64) -> AppResult<Packet> {
    app::packet(&*state.conn()?, job_id)
}
#[tauri::command]
pub fn decide_job(state: S, job_id: i64, decision: String, note: String) -> AppResult<Job> {
    app::decide(&*state.conn()?, job_id, &decision, &note)
}
#[tauri::command]
pub fn save_job_answer(state: S, answer: JobAnswer) -> AppResult<i64> {
    app::save_answer(&*state.conn()?, &answer)
}
#[tauri::command]
pub fn delete_job_answer(state: S, id: i64) -> AppResult<()> {
    app::delete_answer(&*state.conn()?, id)
}
#[tauri::command]
pub fn answer_bank(state: S) -> AppResult<Vec<BankAnswer>> {
    app::answer_bank(&*state.conn()?)
}
#[tauri::command]
pub fn save_bank_answer(state: S, key: String, answer: String) -> AppResult<()> {
    app::save_bank_answer(&*state.conn()?, &key, &answer)
}
/// Opens the official application page in your browser. The app never submits anything itself.
#[tauri::command]
pub fn start_application(state: S, job_id: i64) -> AppResult<String> {
    let url = app::start_application(&*state.conn()?, job_id)?;
    std::process::Command::new("open").arg(&url).spawn().map_err(AppError::other)?;
    Ok(url)
}
#[tauri::command]
pub fn set_follow_up_days(state: S, days: i64) -> AppResult<()> {
    app::set_follow_up_days(&*state.conn()?, days)
}

// ── company research ──
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanyResearchView {
    pub research: Option<crate::company::CompanyResearch>,
    pub suggested_website: Option<String>,
}

#[tauri::command]
pub fn company_research(state: S, job_id: i64) -> AppResult<CompanyResearchView> {
    let c = state.conn()?;
    let job = db::get_job(&c, job_id)?;
    Ok(CompanyResearchView { research: crate::company::get(&c, &job.draft.employer)?, suggested_website: crate::company::suggest_website(&job) })
}

/// Reads the employer's public website (only when the user clicks), keeping earlier choices.
#[tauri::command]
pub async fn research_company(state: State<'_, AppState>, job_id: i64, website: String) -> AppResult<crate::company::CompanyResearch> {
    let job = db::get_job(&*state.conn()?, job_id)?;
    let result = crate::company::read_site(&website, &job.draft.employer, &job.draft.title).await;
    let c = state.conn()?;
    match result {
        Ok(mut r) => {
            if let Some(old) = crate::company::get(&c, &job.draft.employer)? {
                r.terms = crate::company::merge_previous(r.terms, &old.terms);
            }
            crate::company::save(&c, &r)?;
            db::log(&c, "info", "company", &format!("Read {} pages of {} for {}: {} terms found", r.pages.len(), r.website, r.employer, r.terms.len()));
            Ok(r)
        }
        Err(e) => {
            db::log(&c, "error", "company", &format!("Could not read {website}: {e}"));
            Err(e)
        }
    }
}

#[tauri::command]
pub fn save_company_terms(state: S, job_id: i64, terms: Vec<crate::company::CompanyTerm>) -> AppResult<crate::company::CompanyResearch> {
    let c = state.conn()?;
    let job = db::get_job(&c, job_id)?;
    crate::company::save_terms(&c, &job.draft.employer, terms)
}
