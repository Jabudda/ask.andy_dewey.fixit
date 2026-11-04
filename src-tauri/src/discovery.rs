//! Discovery runs: fetch sources → filter (relevance, recency, expiry) → mandatory-rule check →
//! duplicate check → save as "Discovered" → score. Every run is recorded in `search_runs`.

use crate::connectors;
use crate::db;
use crate::eligibility;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::scoring;
use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

/// Shared public job boards are polled at most this often.
pub const REMOTIVE_MIN_GAP_HOURS: i64 = 4;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub source_id: i64,
    pub label: String,
    pub found: i64,
    pub new_jobs: i64,
    pub filtered: i64,
    pub excluded: i64,
    pub duplicates: i64,
    pub skipped_reason: Option<String>,
    pub error: Option<String>,
    pub new_titles: Vec<String>,
}

// ───────────────────────────── sources ─────────────────────────────

fn source_from_row(r: &rusqlite::Row) -> rusqlite::Result<JobSource> {
    Ok(JobSource {
        id: r.get("id")?,
        kind: r.get("kind")?,
        identifier: r.get("identifier")?,
        label: r.get("label")?,
        enabled: r.get::<_, i64>("enabled")? != 0,
        last_run_at: r.get("last_run_at")?,
        last_status: r.get("last_status")?,
        last_error: r.get("last_error")?,
    })
}

pub fn list_sources(c: &Connection) -> AppResult<Vec<JobSource>> {
    let mut st = c.prepare("SELECT * FROM job_sources ORDER BY kind = 'remotive' DESC, label COLLATE NOCASE")?;
    let v = st.query_map([], source_from_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn save_source(c: &Connection, s: &JobSource) -> AppResult<i64> {
    if !connectors::KINDS.contains(&s.kind.as_str()) {
        return Err(AppError::invalid("Source type must be Greenhouse, Lever, Ashby, Remotive, Himalayas or Jobicy"));
    }
    let ident = s.identifier.trim().trim_matches('/').to_lowercase();
    if !connectors::KEYWORD_KINDS.contains(&s.kind.as_str()) && (ident.is_empty() || !ident.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')) {
        return Err(AppError::invalid("Enter the company's board name, e.g. “smartsheet” from boards.greenhouse.io/smartsheet"));
    }
    match s.id {
        Some(id) => {
            c.execute(
                "UPDATE job_sources SET kind=?1, identifier=?2, label=?3, enabled=?4 WHERE id=?5",
                params![s.kind, ident, s.label.trim(), s.enabled as i64, id],
            )?;
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO job_sources (kind, identifier, label, enabled, created_at) VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(kind, identifier) DO UPDATE SET label = excluded.label, enabled = excluded.enabled",
                params![s.kind, ident, s.label.trim(), s.enabled as i64, db::now()],
            )?;
            Ok(c.query_row("SELECT id FROM job_sources WHERE kind=?1 AND identifier=?2", params![s.kind, ident], |r| r.get(0))?)
        }
    }
}

pub fn delete_source(c: &Connection, id: i64) -> AppResult<()> {
    c.execute("DELETE FROM job_sources WHERE id = ?1", [id])?;
    Ok(())
}

pub fn list_runs(c: &Connection, limit: i64) -> AppResult<Vec<SearchRun>> {
    let mut st = c.prepare(
        "SELECT id, source_id, connector, trigger, started_at, finished_at, found, new_jobs, filtered, excluded, duplicates, error
         FROM search_runs ORDER BY id DESC LIMIT ?1",
    )?;
    let v = st
        .query_map([limit], |r| {
            Ok(SearchRun {
                id: r.get(0)?,
                source_id: r.get(1)?,
                connector: r.get(2)?,
                trigger: r.get(3)?,
                started_at: r.get(4)?,
                finished_at: r.get(5)?,
                found: r.get(6)?,
                new_jobs: r.get(7)?,
                filtered: r.get(8)?,
                excluded: r.get(9)?,
                duplicates: r.get(10)?,
                error: r.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

// ───────────────────────────── filters ─────────────────────────────

/// Worth showing the applicant? Title alignment, or several of their keywords in the posting.
pub fn relevant(d: &JobDraft, p: &Preferences) -> bool {
    let title_hit = p.target_titles.iter().any(|t| scoring::title_similarity(&d.title, t) >= 0.5)
        || p.preferred_fields.iter().chain(p.keywords.iter()).any(|k| k.len() >= 4 && crate::documents::term_regex(k).is_match(&d.title));
    if title_hit {
        return true;
    }
    let text = format!("{}\n{}", d.title, d.description);
    let hits = p.keywords.iter().filter(|k| k.len() >= 3 && crate::documents::term_regex(k).is_match(&text)).count();
    let loose_title = p.target_titles.iter().any(|t| scoring::title_similarity(&d.title, t) >= 0.25);
    hits >= 4 || (hits >= 2 && loose_title)
}

fn too_old(d: &JobDraft, p: &Preferences, today: NaiveDate) -> bool {
    d.posted_date
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .map(|pd| (today - pd).num_days() > p.max_posting_age_days.max(1))
        .unwrap_or(false)
}

fn parse_ts(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
}

/// Reason to skip this source right now, if any.
pub fn should_skip(src: &JobSource, now: NaiveDateTime) -> Option<String> {
    if !src.enabled {
        return Some("Source is turned off".into());
    }
    if connectors::KEYWORD_KINDS.contains(&src.kind.as_str()) {
        if let Some(last) = src.last_run_at.as_deref().and_then(parse_ts) {
            let next = last + Duration::hours(REMOTIVE_MIN_GAP_HOURS);
            if now < next && src.last_status.as_deref() == Some("ok") {
                return Some(format!("Checked recently; public boards are polled at most every {REMOTIVE_MIN_GAP_HOURS} hours (next after {})", next.format("%H:%M")));
            }
        }
    }
    None
}

/// Saves the results of one fetched source and records the run.
pub fn process(c: &Connection, src: &JobSource, trigger: &str, started: &str, fetched: Result<Vec<JobDraft>, String>) -> AppResult<RunSummary> {
    let p = db::get_preferences(c)?;
    let today = db::today();
    let id = src.id.unwrap_or(0);
    let mut s = RunSummary {
        source_id: id,
        label: connectors::company_label(src),
        found: 0,
        new_jobs: 0,
        filtered: 0,
        excluded: 0,
        duplicates: 0,
        skipped_reason: None,
        error: None,
        new_titles: vec![],
    };
    match fetched {
        Err(e) => s.error = Some(e),
        Ok(mut drafts) => {
            // Newest first, so recent postings win any tie.
            drafts.sort_by(|a, b| b.posted_date.cmp(&a.posted_date));
            s.found = drafts.len() as i64;
            for d in drafts {
                if !relevant(&d, &p) || too_old(&d, &p, today) || eligibility::is_expired(d.valid_through.as_deref(), today) {
                    s.filtered += 1;
                    continue;
                }
                if !eligibility::evaluate(&d, &p, today).excluded_reasons.is_empty() {
                    s.excluded += 1;
                    continue;
                }
                match db::insert_job(c, &d, false, false) {
                    Ok(r) if r.id.is_some() => {
                        s.new_jobs += 1;
                        s.new_titles.push(format!("{} — {}", d.employer, d.title));
                    }
                    Ok(_) => s.duplicates += 1,
                    Err(e) => db::log(c, "warn", "discovery", &format!("Skipped “{}”: {e}", d.title)),
                }
            }
        }
    }
    let finished = db::now();
    c.execute(
        "INSERT INTO search_runs (connector, source_id, trigger, started_at, finished_at, found, new_jobs, filtered, excluded, duplicates, error)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![src.kind, src.id, trigger, started, finished, s.found, s.new_jobs, s.filtered, s.excluded, s.duplicates, s.error],
    )?;
    c.execute(
        "UPDATE job_sources SET last_run_at = ?1, last_status = ?2, last_error = ?3 WHERE id = ?4",
        params![finished, if s.error.is_some() { "error" } else { "ok" }, s.error, src.id],
    )?;
    match &s.error {
        Some(e) => db::log(c, "error", "discovery", &format!("{} ({}) failed: {e}", s.label, src.kind)),
        None => db::log(
            c,
            "info",
            "discovery",
            &format!("{}: {} found, {} new, {} not relevant/old, {} failed requirements, {} duplicates", s.label, s.found, s.new_jobs, s.filtered, s.excluded, s.duplicates),
        ),
    }
    Ok(s)
}

/// Runs the given source (or every enabled one) against a database file. Network work happens
/// without holding any database connection; results are written afterwards.
pub async fn run_with_path(db_path: &Path, only: Option<i64>, trigger: &str) -> AppResult<Vec<RunSummary>> {
    let (sources, prefs) = {
        let c = db::open(db_path)?;
        (list_sources(&c)?, db::get_preferences(&c)?)
    };
    let client = connectors::client();
    let now = Local::now().naive_local();
    let mut out = vec![];
    for src in sources.iter().filter(|s| only.map(|o| s.id == Some(o)).unwrap_or(true)) {
        if let Some(reason) = should_skip(src, now) {
            if only.is_some() || src.enabled {
                out.push(RunSummary {
                    source_id: src.id.unwrap_or(0),
                    label: connectors::company_label(src),
                    found: 0,
                    new_jobs: 0,
                    filtered: 0,
                    excluded: 0,
                    duplicates: 0,
                    skipped_reason: Some(reason),
                    error: None,
                    new_titles: vec![],
                });
            }
            continue;
        }
        let started = db::now();
        let fetched = connectors::fetch(&client, src, &prefs.search_queries).await;
        let c = db::open(db_path)?;
        out.push(process(&c, src, trigger, &started, fetched)?);
    }
    Ok(out)
}

// ───────────────────────────── schedule ─────────────────────────────

pub fn interval_hours(c: &Connection) -> AppResult<i64> {
    Ok(db::get_setting(c, "search_interval_hours")?.and_then(|v| v.parse().ok()).unwrap_or(12))
}

pub fn set_interval_hours(c: &Connection, h: i64) -> AppResult<()> {
    if !(0..=168).contains(&h) {
        return Err(AppError::invalid("Choose between 0 (off) and 168 hours"));
    }
    db::set_setting(c, "search_interval_hours", &h.to_string())?;
    db::log(c, "info", "schedule", &if h == 0 { "Scheduled searches turned off".to_string() } else { format!("Scheduled searches every {h} hours while the app is open") });
    Ok(())
}

pub fn last_scheduled(c: &Connection) -> AppResult<Option<String>> {
    db::get_setting(c, "last_scheduled_search")
}

/// True when a scheduled run is due now.
pub fn due(c: &Connection, now: NaiveDateTime) -> AppResult<bool> {
    let h = interval_hours(c)?;
    if h == 0 {
        return Ok(false);
    }
    Ok(match last_scheduled(c)?.as_deref().and_then(parse_ts) {
        Some(last) => now >= last + Duration::hours(h),
        None => true,
    })
}

pub fn mark_scheduled(c: &Connection) -> AppResult<()> {
    db::set_setting(c, "last_scheduled_search", &db::now())
}

pub fn get_source(c: &Connection, id: i64) -> AppResult<Option<JobSource>> {
    Ok(c.query_row("SELECT * FROM job_sources WHERE id = ?1", [id], source_from_row).optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(title: &str, desc: &str, posted: &str) -> JobDraft {
        JobDraft {
            title: title.into(),
            employer: "Acme".into(),
            description: desc.into(),
            remote_type: "remote".into(),
            wisconsin_eligibility: "eligible".into(),
            salary_min: Some(60000.0),
            salary_max: Some(80000.0),
            salary_period: Some("year".into()),
            posted_date: Some(posted.into()),
            posting_url: Some(format!("https://jobs.example.com/{}", title.replace(' ', "-"))),
            source: "greenhouse:acme".into(),
            ..Default::default()
        }
    }

    #[test]
    fn relevance() {
        let p = Preferences::default();
        assert!(relevant(&draft("Smartsheet Administrator", "", "2026-11-01"), &p));
        assert!(relevant(&draft("Operations Coordinator II", "", "2026-11-01"), &p));
        assert!(!relevant(&draft("Senior Backend Engineer", "Go, Kubernetes", "2026-11-01"), &p));
        assert!(relevant(&draft("Solutions Consultant", "Smartsheet, Excel, SharePoint, reporting and stakeholder work", "2026-11-01"), &p));
    }

    #[test]
    fn process_counts_and_records() {
        let c = db::open_in_memory().unwrap();
        let src = JobSource { id: Some(save_source(&c, &JobSource { kind: "greenhouse".into(), identifier: "Acme".into(), enabled: true, ..Default::default() }).unwrap()), kind: "greenhouse".into(), identifier: "acme".into(), enabled: true, ..Default::default() };
        let today = db::today();
        let fresh = (today - Duration::days(2)).to_string();
        let old = (today - Duration::days(90)).to_string();
        let mut hybrid = draft("Project Coordinator", "Hybrid, 3 days per week in office", &fresh);
        hybrid.remote_type = "hybrid".into();
        let drafts = vec![
            draft("Smartsheet Administrator", "Requirements\n• Smartsheet", &fresh),
            draft("Data Analyst", "SQL", &old),
            draft("Backend Engineer", "Rust", &fresh),
            hybrid,
        ];
        let s = process(&c, &src, "manual", &db::now(), Ok(drafts.clone())).unwrap();
        assert_eq!((s.found, s.new_jobs, s.filtered, s.excluded, s.duplicates), (4, 1, 2, 1, 0));
        let s2 = process(&c, &src, "scheduled", &db::now(), Ok(drafts)).unwrap();
        assert_eq!(s2.duplicates, 1);
        let runs = list_runs(&c, 10).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].trigger, "scheduled");
        let job = &db::list_jobs(&c).unwrap()[0];
        assert!(job.score.is_some(), "discovered jobs are scored");
        assert_eq!(list_sources(&c).unwrap().iter().find(|x| x.kind == "greenhouse").unwrap().last_status.as_deref(), Some("ok"));
    }

    #[test]
    fn failures_are_recorded() {
        let c = db::open_in_memory().unwrap();
        let id = save_source(&c, &JobSource { kind: "lever".into(), identifier: "nope".into(), enabled: true, ..Default::default() }).unwrap();
        let src = get_source(&c, id).unwrap().unwrap();
        let s = process(&c, &src, "manual", &db::now(), Err("Board not found (HTTP 404)".into())).unwrap();
        assert!(s.error.is_some());
        let src = get_source(&c, id).unwrap().unwrap();
        assert_eq!(src.last_status.as_deref(), Some("error"));
        assert!(db::list_logs(&c, "failed", 10).unwrap().len() == 1);
    }

    #[test]
    fn sources_and_schedule() {
        let c = db::open_in_memory().unwrap();
        assert_eq!(list_sources(&c).unwrap().iter().filter(|s| connectors::KEYWORD_KINDS.contains(&s.kind.as_str())).count(), 3, "keyword boards are seeded");
        assert!(save_source(&c, &JobSource { kind: "greenhouse".into(), identifier: "bad name!".into(), ..Default::default() }).is_err());
        assert!(save_source(&c, &JobSource { kind: "monster".into(), identifier: "x".into(), ..Default::default() }).is_err());
        assert_eq!(interval_hours(&c).unwrap(), 12);
        let now = Local::now().naive_local();
        assert!(due(&c, now).unwrap());
        mark_scheduled(&c).unwrap();
        assert!(!due(&c, now).unwrap());
        assert!(due(&c, now + Duration::hours(13)).unwrap());
        set_interval_hours(&c, 0).unwrap();
        assert!(!due(&c, now + Duration::hours(100)).unwrap());
        assert!(set_interval_hours(&c, 500).is_err());

        let r = JobSource { kind: "remotive".into(), enabled: true, last_run_at: Some(db::now()), last_status: Some("ok".into()), ..Default::default() };
        assert!(should_skip(&r, now).is_some());
        assert!(should_skip(&r, now + Duration::hours(5)).is_none());
    }
}
