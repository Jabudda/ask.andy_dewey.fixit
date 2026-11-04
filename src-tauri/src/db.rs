//! SQLite repository. All business rules that protect data integrity live here so they are
//! enforced no matter which UI path calls them.

use crate::eligibility::{self, is_expired};
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::parse;
use chrono::{Datelike, Duration, Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::collections::BTreeMap;
use std::path::Path;

const MIGRATIONS: &[&str] = &[include_str!("../migrations/001_init.sql")];
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn open(path: &Path) -> AppResult<Connection> {
    let c = Connection::open(path)?;
    init(&c)?;
    Ok(c)
}

pub fn open_in_memory() -> AppResult<Connection> {
    let c = Connection::open_in_memory()?;
    init(&c)?;
    Ok(c)
}

pub fn init(c: &Connection) -> AppResult<()> {
    c.pragma_update(None, "foreign_keys", "ON")?;
    c.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    migrate(c)
}

pub fn migrate(c: &Connection) -> AppResult<()> {
    let current: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(AppError::invalid(format!(
            "This database was created by a newer version of the app (schema {current}); update the app first."
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let target = i as i64 + 1;
        if current < target {
            let tx = c.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", target)?;
            tx.commit()?;
        }
    }
    Ok(())
}

// ───────────────────────────── logging ─────────────────────────────

pub fn log(c: &Connection, level: &str, source: &str, message: &str) {
    // Logging must never break the operation being logged.
    let _ = c.execute(
        "INSERT INTO app_log (ts, level, source, message) VALUES (?1, ?2, ?3, ?4)",
        params![now(), level, source, message],
    );
}

pub fn list_logs(c: &Connection, query: &str, limit: i64) -> AppResult<Vec<LogEntry>> {
    let like = format!("%{}%", query.trim());
    let mut st = c.prepare(
        "SELECT id, ts, level, source, message FROM app_log
         WHERE ?1 = '%%' OR message LIKE ?1 OR source LIKE ?1 OR level LIKE ?1
         ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![like, limit], |r| {
            Ok(LogEntry { id: r.get(0)?, ts: r.get(1)?, level: r.get(2)?, source: r.get(3)?, message: r.get(4)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ───────────────────────────── settings & preferences ─────────────────────────────

pub fn get_setting(c: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
}

pub fn set_setting(c: &Connection, key: &str, value: &str) -> AppResult<()> {
    c.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_preferences(c: &Connection) -> AppResult<Preferences> {
    Ok(match get_setting(c, "preferences")? {
        Some(s) => serde_json::from_str(&s).unwrap_or_default(),
        None => Preferences::default(),
    })
}

pub fn save_preferences(c: &Connection, p: &Preferences) -> AppResult<usize> {
    if p.min_annual_salary < 0.0 || p.hours_per_year <= 0.0 {
        return Err(AppError::invalid("Salary must be ≥ 0 and hours per year must be > 0"));
    }
    set_setting(c, "preferences", &serde_json::to_string(p).map_err(AppError::other)?)?;
    let n = reevaluate_all(c, p)?;
    log(c, "info", "preferences", &format!("Preferences saved; re-checked {n} jobs"));
    Ok(n)
}

pub fn get_milestone(c: &Connection) -> AppResult<String> {
    Ok(get_setting(c, "milestone_date")?.unwrap_or_else(|| "2027-01-31".into()))
}

pub fn set_milestone(c: &Connection, date: &str) -> AppResult<()> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| AppError::invalid("Use a YYYY-MM-DD date"))?;
    set_setting(c, "milestone_date", date)?;
    log(c, "info", "settings", &format!("Milestone date set to {date}"));
    Ok(())
}

// ───────────────────────────── jobs ─────────────────────────────

fn json_vec(s: String) -> Vec<String> {
    serde_json::from_str(&s).unwrap_or_default()
}
fn to_json(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn job_from_row(r: &Row) -> rusqlite::Result<Job> {
    let valid_through: Option<String> = r.get("valid_through")?;
    Ok(Job {
        id: r.get("id")?,
        expired: is_expired(valid_through.as_deref(), today()),
        draft: JobDraft {
            title: r.get("title")?,
            employer: r.get("employer")?,
            location_text: r.get("location_text")?,
            remote_type: r.get("remote_type")?,
            wisconsin_eligibility: r.get("wisconsin_eligibility")?,
            salary_min: r.get("salary_min")?,
            salary_max: r.get("salary_max")?,
            salary_period: r.get("salary_period")?,
            employment_type: r.get("employment_type")?,
            posted_date: r.get("posted_date")?,
            valid_through,
            apply_url: r.get("apply_url")?,
            posting_url: r.get("posting_url")?,
            description: r.get("description")?,
            source: r.get("source")?,
            extraction_method: r.get("extraction_method")?,
            detection_notes: json_vec(r.get("detection_notes")?),
        },
        salary_flag: r.get("salary_flag")?,
        review_flags: json_vec(r.get("review_flags")?),
        excluded_reasons: json_vec(r.get("excluded_reasons")?),
        status: r.get("status")?,
        score: r.get("score")?,
        assessment: r.get("assessment")?,
        notes: r.get("notes")?,
        submitted_at: r.get("submitted_at")?,
        confirmation_ref: r.get("confirmation_ref")?,
        confirmation_method: r.get("confirmation_method")?,
        is_sample: r.get::<_, i64>("is_sample")? != 0,
        discovered_at: r.get("discovered_at")?,
        updated_at: r.get("updated_at")?,
    })
}

fn clean(d: &JobDraft) -> AppResult<JobDraft> {
    let mut d = d.clone();
    d.title = d.title.trim().to_string();
    d.employer = d.employer.trim().to_string();
    if d.title.is_empty() || d.employer.is_empty() {
        return Err(AppError::invalid("Job title and employer are required"));
    }
    for u in [&d.apply_url, &d.posting_url].into_iter().flatten() {
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            return Err(AppError::invalid(format!("Links must start with http:// or https:// ({u})")));
        }
    }
    let blank = |o: &mut Option<String>| {
        if o.as_deref().map(str::trim).unwrap_or("").is_empty() {
            *o = None;
        }
    };
    blank(&mut d.apply_url);
    blank(&mut d.posting_url);
    blank(&mut d.posted_date);
    blank(&mut d.valid_through);
    if let (Some(lo), Some(hi)) = (d.salary_min, d.salary_max) {
        if lo > hi {
            return Err(AppError::invalid("Minimum salary cannot exceed maximum salary"));
        }
    }
    if (d.salary_min.is_some() || d.salary_max.is_some()) && d.salary_period.is_none() {
        d.salary_period = Some("year".into());
    }
    Ok(d)
}

fn canonical_for(d: &JobDraft) -> Option<String> {
    d.posting_url.as_deref().or(d.apply_url.as_deref()).and_then(parse::canonical_url)
}

pub fn find_duplicate(c: &Connection, d: &JobDraft, exclude_id: Option<i64>) -> AppResult<Option<Job>> {
    let fp = parse::fingerprint(&d.employer, &d.title);
    let curl = canonical_for(d);
    Ok(c.query_row(
        "SELECT * FROM jobs WHERE (fingerprint = ?1 OR fingerprint LIKE ?1 || '#%' OR (?2 IS NOT NULL AND canonical_url = ?2))
           AND id != ?3 ORDER BY id LIMIT 1",
        params![fp, curl, exclude_id.unwrap_or(-1)],
        job_from_row,
    )
    .optional()?)
}

/// Saves a job. If it looks like a duplicate and `force` is false, nothing is written and the
/// existing job is returned so the user can decide.
pub fn insert_job(c: &Connection, draft: &JobDraft, force: bool, is_sample: bool) -> AppResult<SaveJobResult> {
    let d = clean(draft)?;
    if !force {
        if let Some(existing) = find_duplicate(c, &d, None)? {
            log(c, "info", "import", &format!("Duplicate of job #{} detected: {} — {}", existing.id, d.employer, d.title));
            return Ok(SaveJobResult { id: None, duplicate_of: Some(existing) });
        }
    }
    let p = get_preferences(c)?;
    let ev = eligibility::evaluate(&d, &p, today());
    let mut fp = parse::fingerprint(&d.employer, &d.title);
    if force {
        fp = format!("{fp}#{}", Local::now().timestamp_micros());
    }
    let ts = now();
    let tx = c.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO jobs (fingerprint, canonical_url, source, extraction_method, detection_notes, title, employer,
            location_text, remote_type, wisconsin_eligibility, salary_min, salary_max, salary_period, salary_flag,
            employment_type, posted_date, valid_through, apply_url, posting_url, description, review_flags,
            excluded_reasons, status, is_sample, discovered_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,'Discovered',?23,?24,?24)",
        params![
            fp, canonical_for(&d), d.source, d.extraction_method, to_json(&d.detection_notes), d.title, d.employer,
            d.location_text, d.remote_type, d.wisconsin_eligibility, d.salary_min, d.salary_max, d.salary_period,
            ev.salary_flag, d.employment_type, d.posted_date, d.valid_through, d.apply_url, d.posting_url,
            d.description, to_json(&ev.review_flags), to_json(&ev.excluded_reasons), is_sample as i64, ts
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO status_history (job_id, from_status, to_status, note, changed_at) VALUES (?1, NULL, 'Discovered', ?2, ?3)",
        params![id, format!("Added via {}", d.source), ts],
    )?;
    tx.commit()?;
    log(c, "info", "import", &format!("Saved job #{id}: {} — {}", d.employer, d.title));
    Ok(SaveJobResult { id: Some(id), duplicate_of: None })
}

pub fn update_job(c: &Connection, id: i64, draft: &JobDraft) -> AppResult<Job> {
    let d = clean(draft)?;
    if let Some(dup) = find_duplicate(c, &d, Some(id))? {
        return Err(AppError::invalid(format!(
            "Another saved job (#{}: {} — {}) already has this employer/title or link",
            dup.id, dup.draft.employer, dup.draft.title
        )));
    }
    let p = get_preferences(c)?;
    let ev = eligibility::evaluate(&d, &p, today());
    let n = c.execute(
        "UPDATE jobs SET fingerprint=?1, canonical_url=?2, title=?3, employer=?4, location_text=?5, remote_type=?6,
            wisconsin_eligibility=?7, salary_min=?8, salary_max=?9, salary_period=?10, salary_flag=?11,
            employment_type=?12, posted_date=?13, valid_through=?14, apply_url=?15, posting_url=?16, description=?17,
            review_flags=?18, excluded_reasons=?19, updated_at=?20 WHERE id=?21",
        params![
            parse::fingerprint(&d.employer, &d.title), canonical_for(&d), d.title, d.employer, d.location_text,
            d.remote_type, d.wisconsin_eligibility, d.salary_min, d.salary_max, d.salary_period, ev.salary_flag,
            d.employment_type, d.posted_date, d.valid_through, d.apply_url, d.posting_url, d.description,
            to_json(&ev.review_flags), to_json(&ev.excluded_reasons), now(), id
        ],
    )?;
    if n == 0 {
        return Err(AppError::invalid(format!("Job #{id} not found")));
    }
    get_job(c, id)
}

pub fn reevaluate_all(c: &Connection, p: &Preferences) -> AppResult<usize> {
    let jobs = list_jobs(c)?;
    let t = today();
    let tx = c.unchecked_transaction()?;
    for j in &jobs {
        let ev = eligibility::evaluate(&j.draft, p, t);
        tx.execute(
            "UPDATE jobs SET salary_flag=?1, review_flags=?2, excluded_reasons=?3 WHERE id=?4",
            params![ev.salary_flag, to_json(&ev.review_flags), to_json(&ev.excluded_reasons), j.id],
        )?;
    }
    tx.commit()?;
    Ok(jobs.len())
}

pub fn get_job(c: &Connection, id: i64) -> AppResult<Job> {
    c.query_row("SELECT * FROM jobs WHERE id = ?1", [id], job_from_row)
        .optional()?
        .ok_or_else(|| AppError::invalid(format!("Job #{id} not found")))
}

pub fn list_jobs(c: &Connection) -> AppResult<Vec<Job>> {
    let mut st = c.prepare("SELECT * FROM jobs ORDER BY discovered_at DESC, id DESC")?;
    let rows = st.query_map([], job_from_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete_job(c: &Connection, id: i64) -> AppResult<()> {
    let j = get_job(c, id)?;
    c.execute("DELETE FROM jobs WHERE id = ?1", [id])?;
    log(c, "warn", "jobs", &format!("Deleted job #{id}: {} — {}", j.draft.employer, j.draft.title));
    Ok(())
}

pub fn update_notes(c: &Connection, id: i64, notes: &str) -> AppResult<()> {
    c.execute("UPDATE jobs SET notes = ?1, updated_at = ?2 WHERE id = ?3", params![notes, now(), id])?;
    Ok(())
}

/// The single entry point for status changes, with the human-in-the-loop guards.
pub fn change_status(c: &Connection, req: &StatusChange) -> AppResult<Job> {
    if !STATUSES.contains(&req.to_status.as_str()) {
        return Err(AppError::invalid(format!("Unknown status “{}”", req.to_status)));
    }
    let job = get_job(c, req.job_id)?;
    if job.status == req.to_status {
        return Ok(job);
    }
    if req.to_status == "Approved" && job.status != "Awaiting Approval" {
        return Err(AppError::invalid(
            "A job can only be Approved from the Awaiting Approval queue, after you have reviewed it there.",
        ));
    }
    let reference = req.confirmation_ref.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let ts = now();
    let tx = c.unchecked_transaction()?;
    if req.to_status == "Submitted" {
        if !req.user_confirmed_submission && reference.is_none() {
            return Err(AppError::invalid(
                "Mark as Submitted only after the employer confirmed receipt: tick the confirmation box or enter the confirmation/reference number.",
            ));
        }
        let method = if reference.is_some() { "reference_number" } else { "user_confirmed" };
        tx.execute(
            "UPDATE jobs SET submitted_at = COALESCE(submitted_at, ?1), confirmation_ref = COALESCE(?2, confirmation_ref),
                confirmation_method = ?3 WHERE id = ?4",
            params![ts, reference, method, req.job_id],
        )?;
    }
    tx.execute("UPDATE jobs SET status = ?1, updated_at = ?2 WHERE id = ?3", params![req.to_status, ts, req.job_id])?;
    tx.execute(
        "INSERT INTO status_history (job_id, from_status, to_status, note, changed_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![req.job_id, job.status, req.to_status, req.note.trim(), ts],
    )?;
    tx.commit()?;
    log(c, "info", "status", &format!("Job #{}: {} → {}", req.job_id, job.status, req.to_status));
    get_job(c, req.job_id)
}

pub fn get_job_detail(c: &Connection, id: i64) -> AppResult<JobDetail> {
    let job = get_job(c, id)?;
    let history = {
        let mut st = c.prepare(
            "SELECT id, from_status, to_status, note, changed_at FROM status_history WHERE job_id = ?1 ORDER BY changed_at DESC, id DESC",
        )?;
        let v = st
            .query_map([id], |r| {
                Ok(StatusEvent { id: r.get(0)?, from_status: r.get(1)?, to_status: r.get(2)?, note: r.get(3)?, changed_at: r.get(4)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        v
    };
    let contacts = {
        let mut st = c.prepare("SELECT id, job_id, name, role, email, phone, notes FROM contacts WHERE job_id = ?1 ORDER BY id")?;
        let v = st
            .query_map([id], |r| {
                Ok(Contact { id: r.get(0)?, job_id: r.get(1)?, name: r.get(2)?, role: r.get(3)?, email: r.get(4)?, phone: r.get(5)?, notes: r.get(6)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        v
    };
    let interviews = {
        let mut st = c.prepare("SELECT id, job_id, scheduled_at, kind, notes, outcome FROM interviews WHERE job_id = ?1 ORDER BY scheduled_at")?;
        let v = st
            .query_map([id], |r| {
                Ok(Interview { id: r.get(0)?, job_id: r.get(1)?, scheduled_at: r.get(2)?, kind: r.get(3)?, notes: r.get(4)?, outcome: r.get(5)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        v
    };
    let reminders = {
        let mut st = c.prepare("SELECT id, job_id, due_date, note, done FROM reminders WHERE job_id = ?1 ORDER BY done, due_date")?;
        let v = st
            .query_map([id], |r| {
                Ok(Reminder { id: r.get(0)?, job_id: r.get(1)?, due_date: r.get(2)?, note: r.get(3)?, done: r.get::<_, i64>(4)? != 0 })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        v
    };
    let documents = list_documents(c, Some(id))?;
    Ok(JobDetail { job, history, contacts, interviews, reminders, documents })
}

pub fn save_contact(c: &Connection, x: &Contact) -> AppResult<i64> {
    if x.name.trim().is_empty() {
        return Err(AppError::invalid("Contact name is required"));
    }
    match x.id {
        Some(id) => {
            c.execute(
                "UPDATE contacts SET name=?1, role=?2, email=?3, phone=?4, notes=?5 WHERE id=?6",
                params![x.name.trim(), x.role, x.email, x.phone, x.notes, id],
            )?;
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO contacts (job_id, name, role, email, phone, notes) VALUES (?1,?2,?3,?4,?5,?6)",
                params![x.job_id, x.name.trim(), x.role, x.email, x.phone, x.notes],
            )?;
            Ok(c.last_insert_rowid())
        }
    }
}

pub fn save_interview(c: &Connection, x: &Interview) -> AppResult<i64> {
    if x.scheduled_at.trim().len() < 10 {
        return Err(AppError::invalid("Interview date/time is required"));
    }
    let id = match x.id {
        Some(id) => {
            c.execute(
                "UPDATE interviews SET scheduled_at=?1, kind=?2, notes=?3, outcome=?4 WHERE id=?5",
                params![x.scheduled_at, x.kind, x.notes, x.outcome, id],
            )?;
            id
        }
        None => {
            c.execute(
                "INSERT INTO interviews (job_id, scheduled_at, kind, notes, outcome) VALUES (?1,?2,?3,?4,?5)",
                params![x.job_id, x.scheduled_at, x.kind, x.notes, x.outcome],
            )?;
            c.last_insert_rowid()
        }
    };
    log(c, "info", "interviews", &format!("Interview saved for job #{} at {}", x.job_id, x.scheduled_at));
    Ok(id)
}

pub fn save_reminder(c: &Connection, x: &Reminder) -> AppResult<i64> {
    NaiveDate::parse_from_str(x.due_date.get(..10).unwrap_or(""), "%Y-%m-%d")
        .map_err(|_| AppError::invalid("Reminder needs a due date"))?;
    match x.id {
        Some(id) => {
            c.execute(
                "UPDATE reminders SET due_date=?1, note=?2, done=?3 WHERE id=?4",
                params![x.due_date, x.note, x.done as i64, id],
            )?;
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO reminders (job_id, due_date, note, done) VALUES (?1,?2,?3,?4)",
                params![x.job_id, x.due_date, x.note, x.done as i64],
            )?;
            Ok(c.last_insert_rowid())
        }
    }
}

pub fn delete_child(c: &Connection, table: &str, id: i64) -> AppResult<()> {
    let table = match table {
        "contacts" | "interviews" | "reminders" => table,
        _ => return Err(AppError::invalid("Unknown record type")),
    };
    c.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [id])?;
    Ok(())
}

// ───────────────────────────── dashboard ─────────────────────────────

fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

pub fn dashboard_stats(c: &Connection, today: NaiveDate) -> AppResult<DashboardStats> {
    let jobs = list_jobs(c)?;
    let p = get_preferences(c)?;
    let count = |s: &str| jobs.iter().filter(|j| j.status == s).count() as i64;
    let ever = |s: &str| -> AppResult<i64> {
        Ok(c.query_row("SELECT COUNT(DISTINCT job_id) FROM status_history WHERE to_status = ?1", [s], |r| r.get(0))?)
    };
    let submitted = jobs.iter().filter(|j| j.submitted_at.is_some()).count() as i64;
    let interviewed = ever("Interviewing")?;
    let offers_ever = ever("Offer")?;

    let status_counts: Vec<LabelCount> =
        STATUSES.iter().map(|s| LabelCount { label: s.to_string(), count: count(s) }).collect();

    let buckets = [("90–100", 90, 100), ("75–89", 75, 89), ("60–74", 60, 74), ("40–59", 40, 59), ("0–39", 0, 39)];
    let mut score_buckets: Vec<LabelCount> = buckets
        .iter()
        .map(|(l, lo, hi)| LabelCount {
            label: l.to_string(),
            count: jobs.iter().filter(|j| j.score.map(|s| s >= *lo && s <= *hi).unwrap_or(false)).count() as i64,
        })
        .collect();
    let unscored = jobs.iter().filter(|j| j.score.is_none()).count() as i64;
    score_buckets.push(LabelCount { label: "Unscored".into(), count: unscored });

    // 12 weeks of activity, oldest first.
    let this_week = week_start(today);
    let mut weeks: BTreeMap<NaiveDate, (i64, i64, i64)> =
        (0..12).map(|i| (this_week - Duration::weeks(11 - i), (0, 0, 0))).collect();
    let bump = |weeks: &mut BTreeMap<NaiveDate, (i64, i64, i64)>, day: Option<NaiveDate>, which: usize| {
        if let Some(d) = day {
            if let Some(e) = weeks.get_mut(&week_start(d)) {
                match which {
                    0 => e.0 += 1,
                    1 => e.1 += 1,
                    _ => e.2 += 1,
                }
            }
        }
    };
    for j in &jobs {
        bump(&mut weeks, parse_day(&j.discovered_at), 0);
        bump(&mut weeks, j.submitted_at.as_deref().and_then(parse_day), 1);
    }
    let interview_dates: Vec<String> = {
        let mut st = c.prepare("SELECT scheduled_at FROM interviews")?;
        let v = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
        v
    };
    for s in &interview_dates {
        bump(&mut weeks, parse_day(s), 2);
    }
    let activity = weeks
        .into_iter()
        .map(|(w, (d, s, i))| WeeklyActivity { week_start: w.to_string(), discovered: d, submitted: s, interviews: i })
        .collect();

    let today_s = today.to_string();
    let interviews_scheduled: i64 =
        c.query_row("SELECT COUNT(*) FROM interviews WHERE substr(scheduled_at,1,10) >= ?1", [&today_s], |r| r.get(0))?;

    // Follow-ups: due reminders + submitted applications with no update for N days.
    let mut follow_ups = Vec::new();
    {
        let mut st = c.prepare(
            "SELECT r.job_id, j.title, j.employer, r.note, r.due_date FROM reminders r JOIN jobs j ON j.id = r.job_id
             WHERE r.done = 0 AND substr(r.due_date,1,10) <= ?1 ORDER BY r.due_date",
        )?;
        let rows = st.query_map([&today_s], |r| {
            Ok(FollowUp {
                job_id: r.get(0)?,
                title: r.get(1)?,
                employer: r.get(2)?,
                reason: {
                    let n: String = r.get(3)?;
                    if n.is_empty() { "Reminder due".to_string() } else { n }
                },
                due: r.get(4)?,
            })
        })?;
        for f in rows {
            follow_ups.push(f?);
        }
    }
    let stale_before = today - Duration::days(p.follow_up_days.max(1));
    for j in jobs.iter().filter(|j| j.status == "Submitted") {
        if parse_day(&j.updated_at).map(|d| d <= stale_before).unwrap_or(false)
            && !follow_ups.iter().any(|f| f.job_id == j.id)
        {
            follow_ups.push(FollowUp {
                job_id: j.id,
                title: j.draft.title.clone(),
                employer: j.draft.employer.clone(),
                reason: format!("No update {}+ days after submission", p.follow_up_days),
                due: j.updated_at.get(..10).unwrap_or("").to_string(),
            });
        }
    }
    let needs_follow_up = {
        let mut ids: Vec<i64> = follow_ups.iter().map(|f| f.job_id).collect();
        ids.sort();
        ids.dedup();
        ids.len() as i64
    };

    let rate = |n: i64| if submitted > 0 { Some(n as f64 / submitted as f64) } else { None };
    Ok(DashboardStats {
        total_jobs: jobs.len() as i64,
        strong_matches: jobs.iter().filter(|j| j.score.map(|s| s >= 75).unwrap_or(false)).count() as i64,
        unscored,
        awaiting_approval: count("Awaiting Approval"),
        submitted,
        interviews_scheduled,
        offers: offers_ever,
        rejections: count("Rejected"),
        needs_follow_up,
        excluded: jobs.iter().filter(|j| !j.excluded_reasons.is_empty()).count() as i64,
        needs_review: jobs.iter().filter(|j| j.excluded_reasons.is_empty() && !j.review_flags.is_empty()).count() as i64,
        status_counts,
        score_buckets,
        activity,
        follow_ups,
        interview_rate: rate(interviewed),
        offer_rate: rate(offers_ever),
        milestone_date: get_milestone(c)?,
    })
}

// ───────────────────────────── profile ─────────────────────────────

pub fn get_profile(c: &Connection) -> AppResult<ProfileBundle> {
    let basics = c.query_row(
        "SELECT full_name, email, phone, location, linkedin_url, headline, summary, updated_at FROM profile WHERE id = 1",
        [],
        |r| {
            Ok(ProfileBasics {
                full_name: r.get(0)?,
                email: r.get(1)?,
                phone: r.get(2)?,
                location: r.get(3)?,
                linkedin_url: r.get(4)?,
                headline: r.get(5)?,
                summary: r.get(6)?,
                updated_at: r.get(7)?,
            })
        },
    )?;
    let experience = {
        let mut st = c.prepare(
            "SELECT id, employer, title, location, start_date, end_date, is_current, achievements, sort_order
             FROM experience ORDER BY is_current DESC, start_date DESC, sort_order",
        )?;
        let v = st
            .query_map([], |r| {
                Ok(Experience {
                    id: r.get(0)?,
                    employer: r.get(1)?,
                    title: r.get(2)?,
                    location: r.get(3)?,
                    start_date: r.get(4)?,
                    end_date: r.get(5)?,
                    is_current: r.get::<_, i64>(6)? != 0,
                    achievements: r.get(7)?,
                    sort_order: r.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        v
    };
    let skills = {
        let mut st = c.prepare("SELECT id, name, category, evidence FROM skills ORDER BY category, name")?;
        let v = st
            .query_map([], |r| Ok(Skill { id: r.get(0)?, name: r.get(1)?, category: r.get(2)?, evidence: r.get(3)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        v
    };
    let certifications = {
        let mut st = c.prepare("SELECT id, name, issuer, date_earned, expires, credential_id FROM certifications ORDER BY date_earned DESC")?;
        let v = st
            .query_map([], |r| {
                Ok(Certification { id: r.get(0)?, name: r.get(1)?, issuer: r.get(2)?, date_earned: r.get(3)?, expires: r.get(4)?, credential_id: r.get(5)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        v
    };
    let accomplishments = {
        let mut st = c.prepare("SELECT id, title, description, evidence FROM accomplishments ORDER BY id")?;
        let v = st
            .query_map([], |r| Ok(Accomplishment { id: r.get(0)?, title: r.get(1)?, description: r.get(2)?, evidence: r.get(3)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        v
    };
    Ok(ProfileBundle { basics, experience, skills, certifications, accomplishments, documents: list_documents(c, None)? })
}

pub fn save_profile_basics(c: &Connection, b: &ProfileBasics) -> AppResult<()> {
    c.execute(
        "UPDATE profile SET full_name=?1, email=?2, phone=?3, location=?4, linkedin_url=?5, headline=?6, summary=?7, updated_at=?8 WHERE id=1",
        params![b.full_name.trim(), b.email.trim(), b.phone.trim(), b.location.trim(), b.linkedin_url.trim(), b.headline.trim(), b.summary.trim(), now()],
    )?;
    Ok(())
}

pub fn save_experience(c: &Connection, x: &Experience) -> AppResult<i64> {
    if x.employer.trim().is_empty() || x.title.trim().is_empty() {
        return Err(AppError::invalid("Employer and job title are required"));
    }
    match x.id {
        Some(id) => {
            c.execute(
                "UPDATE experience SET employer=?1, title=?2, location=?3, start_date=?4, end_date=?5, is_current=?6, achievements=?7, sort_order=?8 WHERE id=?9",
                params![x.employer.trim(), x.title.trim(), x.location, x.start_date, x.end_date, x.is_current as i64, x.achievements, x.sort_order, id],
            )?;
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO experience (employer, title, location, start_date, end_date, is_current, achievements, sort_order) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![x.employer.trim(), x.title.trim(), x.location, x.start_date, x.end_date, x.is_current as i64, x.achievements, x.sort_order],
            )?;
            Ok(c.last_insert_rowid())
        }
    }
}

pub fn save_skill(c: &Connection, x: &Skill) -> AppResult<i64> {
    let name = x.name.trim();
    if name.is_empty() {
        return Err(AppError::invalid("Skill name is required"));
    }
    match x.id {
        Some(id) => {
            c.execute("UPDATE skills SET name=?1, category=?2, evidence=?3 WHERE id=?4", params![name, x.category, x.evidence, id])?;
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO skills (name, category, evidence) VALUES (?1,?2,?3) ON CONFLICT(name) DO UPDATE SET
                   category = CASE WHEN excluded.category != '' THEN excluded.category ELSE skills.category END,
                   evidence = CASE WHEN excluded.evidence != '' THEN excluded.evidence ELSE skills.evidence END",
                params![name, x.category, x.evidence],
            )?;
            Ok(c.query_row("SELECT id FROM skills WHERE name = ?1", [name], |r| r.get(0))?)
        }
    }
}

pub fn save_certification(c: &Connection, x: &Certification) -> AppResult<i64> {
    if x.name.trim().is_empty() {
        return Err(AppError::invalid("Certification name is required"));
    }
    match x.id {
        Some(id) => {
            c.execute(
                "UPDATE certifications SET name=?1, issuer=?2, date_earned=?3, expires=?4, credential_id=?5 WHERE id=?6",
                params![x.name.trim(), x.issuer, x.date_earned, x.expires, x.credential_id, id],
            )?;
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO certifications (name, issuer, date_earned, expires, credential_id) VALUES (?1,?2,?3,?4,?5)",
                params![x.name.trim(), x.issuer, x.date_earned, x.expires, x.credential_id],
            )?;
            Ok(c.last_insert_rowid())
        }
    }
}

pub fn save_accomplishment(c: &Connection, x: &Accomplishment) -> AppResult<i64> {
    if x.title.trim().is_empty() {
        return Err(AppError::invalid("Accomplishment title is required"));
    }
    match x.id {
        Some(id) => {
            c.execute(
                "UPDATE accomplishments SET title=?1, description=?2, evidence=?3 WHERE id=?4",
                params![x.title.trim(), x.description, x.evidence, id],
            )?;
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO accomplishments (title, description, evidence) VALUES (?1,?2,?3)",
                params![x.title.trim(), x.description, x.evidence],
            )?;
            Ok(c.last_insert_rowid())
        }
    }
}

pub fn delete_profile_item(c: &Connection, table: &str, id: i64) -> AppResult<()> {
    let table = match table {
        "experience" | "skills" | "certifications" | "accomplishments" => table,
        _ => return Err(AppError::invalid("Unknown profile section")),
    };
    c.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [id])?;
    Ok(())
}

// ───────────────────────────── documents ─────────────────────────────

fn doc_from_row(r: &Row) -> rusqlite::Result<DocumentRec> {
    Ok(DocumentRec {
        id: r.get("id")?,
        kind: r.get("kind")?,
        file_name: r.get("file_name")?,
        stored_path: r.get("stored_path")?,
        sha256: r.get("sha256")?,
        extracted_text: r.get("extracted_text")?,
        job_id: r.get("job_id")?,
        version: r.get("version")?,
        created_at: r.get("created_at")?,
    })
}

pub fn insert_document(
    c: &Connection,
    kind: &str,
    file_name: &str,
    stored_path: &str,
    sha: &str,
    text: &str,
    job_id: Option<i64>,
) -> AppResult<DocumentRec> {
    let version: i64 = c.query_row(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM documents WHERE kind = ?1 AND job_id IS ?2",
        params![kind, job_id],
        |r| r.get(0),
    )?;
    c.execute(
        "INSERT INTO documents (kind, file_name, stored_path, sha256, extracted_text, job_id, version, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![kind, file_name, stored_path, sha, text, job_id, version, now()],
    )?;
    let id = c.last_insert_rowid();
    Ok(c.query_row("SELECT * FROM documents WHERE id = ?1", [id], doc_from_row)?)
}

pub fn get_document(c: &Connection, id: i64) -> AppResult<DocumentRec> {
    c.query_row("SELECT * FROM documents WHERE id = ?1", [id], doc_from_row)
        .optional()?
        .ok_or_else(|| AppError::invalid("Document not found"))
}

pub fn list_documents(c: &Connection, job_id: Option<i64>) -> AppResult<Vec<DocumentRec>> {
    let mut st = c.prepare("SELECT * FROM documents WHERE job_id IS ?1 ORDER BY kind, version DESC, id DESC")?;
    let rows = st.query_map([job_id], doc_from_row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete_document(c: &Connection, id: i64) -> AppResult<DocumentRec> {
    let d = get_document(c, id)?;
    c.execute("DELETE FROM documents WHERE id = ?1", [id])?;
    log(c, "warn", "documents", &format!("Removed document #{id} ({})", d.file_name));
    Ok(d)
}

// ───────────────────────────── sample data ─────────────────────────────

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SampleJob {
    #[serde(flatten)]
    draft: JobDraft,
    days_ago: i64,
    #[serde(default)]
    path: Vec<String>,
    #[serde(default)]
    interview_in_days: Option<i64>,
    #[serde(default)]
    reminder_in_days: Option<i64>,
}

pub const SAMPLE_JSON: &str = include_str!("../../sample-data/sample_jobs.json");

/// Loads clearly-labelled sample jobs (is_sample = 1, employer names end in "(SAMPLE)").
pub fn load_sample_data(c: &Connection) -> AppResult<usize> {
    let samples: Vec<SampleJob> = serde_json::from_str(SAMPLE_JSON).map_err(AppError::other)?;
    let mut n = 0;
    for (i, s) in samples.iter().enumerate() {
        let res = insert_job(c, &s.draft, false, true)?;
        let Some(id) = res.id else { continue };
        n += 1;
        for st in &s.path {
            change_status(
                c,
                &StatusChange {
                    job_id: id,
                    to_status: st.clone(),
                    note: "Sample data".into(),
                    confirmation_ref: (st == "Submitted").then(|| format!("SAMPLE-REF-{}", 1000 + i)),
                    user_confirmed_submission: false,
                },
            )?;
        }
        // Spread sample history across past weeks so charts have shape.
        let discovered = (Local::now() - Duration::days(s.days_ago)).format("%Y-%m-%d %H:%M:%S").to_string();
        let progressed = (Local::now() - Duration::days((s.days_ago - 3).max(0))).format("%Y-%m-%d %H:%M:%S").to_string();
        c.execute("UPDATE jobs SET discovered_at = ?1, updated_at = ?2 WHERE id = ?3", params![discovered, progressed, id])?;
        c.execute("UPDATE jobs SET submitted_at = ?1 WHERE id = ?2 AND submitted_at IS NOT NULL", params![progressed, id])?;
        c.execute("UPDATE status_history SET changed_at = ?1 WHERE job_id = ?2 AND from_status IS NULL", params![discovered, id])?;
        c.execute("UPDATE status_history SET changed_at = ?1 WHERE job_id = ?2 AND from_status IS NOT NULL", params![progressed, id])?;
        if let Some(d) = s.interview_in_days {
            let at = (Local::now() + Duration::days(d)).format("%Y-%m-%dT10:00").to_string();
            save_interview(c, &Interview { id: None, job_id: id, scheduled_at: at, kind: "Video — SAMPLE".into(), notes: String::new(), outcome: String::new() })?;
        }
        if let Some(d) = s.reminder_in_days {
            let due = (today() + Duration::days(d)).to_string();
            save_reminder(c, &Reminder { id: None, job_id: id, due_date: due, note: "Follow up with recruiter (SAMPLE)".into(), done: false })?;
        }
    }
    log(c, "info", "sample", &format!("Loaded {n} sample jobs"));
    Ok(n)
}

pub fn remove_sample_data(c: &Connection) -> AppResult<usize> {
    let n = c.execute("DELETE FROM jobs WHERE is_sample = 1", [])?;
    log(c, "info", "sample", &format!("Removed {n} sample jobs"));
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(employer: &str, title: &str) -> JobDraft {
        JobDraft {
            title: title.into(),
            employer: employer.into(),
            remote_type: "remote".into(),
            wisconsin_eligibility: "eligible".into(),
            salary_min: Some(60000.0),
            salary_max: Some(70000.0),
            salary_period: Some("year".into()),
            posting_url: Some("https://jobs.example.com/a/1?utm_source=x".into()),
            ..Default::default()
        }
    }

    #[test]
    fn migrations_are_idempotent() {
        let c = open_in_memory().unwrap();
        migrate(&c).unwrap();
        let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(get_milestone(&c).unwrap(), "2027-01-31");
    }

    #[test]
    fn insert_and_detect_duplicates() {
        let c = open_in_memory().unwrap();
        let r = insert_job(&c, &draft("Fabrikam, Inc.", "Data Analyst"), false, false).unwrap();
        let id = r.id.unwrap();
        // Same employer/title, different formatting
        let dup = insert_job(&c, &draft("FABRIKAM", "data analyst"), false, false).unwrap();
        assert_eq!(dup.id, None);
        assert_eq!(dup.duplicate_of.unwrap().id, id);
        // Same URL, different title
        let mut other = draft("Somebody", "Other Title");
        other.posting_url = Some("https://jobs.example.com/a/1/".into());
        assert!(insert_job(&c, &other, false, false).unwrap().duplicate_of.is_some());
        // Force save keeps both
        let forced = insert_job(&c, &draft("Fabrikam", "Data Analyst"), true, false).unwrap();
        assert!(forced.id.is_some());
        assert_eq!(list_jobs(&c).unwrap().len(), 2);
        let detail = get_job_detail(&c, id).unwrap();
        assert_eq!(detail.history.len(), 1);
        assert_eq!(detail.job.salary_flag, "meets");
    }

    #[test]
    fn validation() {
        let c = open_in_memory().unwrap();
        assert!(insert_job(&c, &draft("", "X"), false, false).is_err());
        let mut d = draft("A", "B");
        d.apply_url = Some("javascript:alert(1)".into());
        assert!(insert_job(&c, &d, false, false).is_err());
        let mut d = draft("A", "B");
        d.salary_min = Some(90000.0);
        assert!(insert_job(&c, &d, false, false).is_err());
    }

    #[test]
    fn status_guards() {
        let c = open_in_memory().unwrap();
        let id = insert_job(&c, &draft("Contoso", "Ops Analyst"), false, false).unwrap().id.unwrap();
        let ch = |to: &str, confirm: bool, r: Option<&str>| StatusChange {
            job_id: id,
            to_status: to.into(),
            note: String::new(),
            confirmation_ref: r.map(String::from),
            user_confirmed_submission: confirm,
        };
        assert!(change_status(&c, &ch("Approved", false, None)).is_err(), "approval must come from queue");
        assert!(change_status(&c, &ch("Submitted", false, None)).is_err(), "submission needs confirmation");
        assert!(change_status(&c, &ch("Submitted", false, Some("   "))).is_err(), "blank reference is not confirmation");
        assert!(change_status(&c, &ch("Bogus", true, None)).is_err());
        change_status(&c, &ch("Awaiting Approval", false, None)).unwrap();
        change_status(&c, &ch("Approved", false, None)).unwrap();
        let j = change_status(&c, &ch("Submitted", false, Some("REF-123"))).unwrap();
        assert_eq!(j.confirmation_ref.as_deref(), Some("REF-123"));
        assert_eq!(j.confirmation_method.as_deref(), Some("reference_number"));
        assert!(j.submitted_at.is_some());
        let first_submit = j.submitted_at.clone();
        change_status(&c, &ch("Interviewing", false, None)).unwrap();
        let j = change_status(&c, &ch("Submitted", true, None)).unwrap();
        assert_eq!(j.submitted_at, first_submit, "original submission date is preserved");
        assert_eq!(get_job_detail(&c, id).unwrap().history.len(), 6);
    }

    #[test]
    fn preferences_reevaluate_existing_jobs() {
        let c = open_in_memory().unwrap();
        let id = insert_job(&c, &draft("Contoso", "Ops Analyst"), false, false).unwrap().id.unwrap();
        assert!(get_job(&c, id).unwrap().excluded_reasons.is_empty());
        let mut p = get_preferences(&c).unwrap();
        p.min_annual_salary = 80000.0;
        save_preferences(&c, &p).unwrap();
        let j = get_job(&c, id).unwrap();
        assert_eq!(j.salary_flag, "below");
        assert_eq!(j.excluded_reasons.len(), 1);
    }

    #[test]
    fn milestone_change_keeps_history() {
        let c = open_in_memory().unwrap();
        let id = insert_job(&c, &draft("Contoso", "Ops Analyst"), false, false).unwrap().id.unwrap();
        let before = get_job_detail(&c, id).unwrap();
        set_milestone(&c, "2027-03-15").unwrap();
        assert!(set_milestone(&c, "next spring").is_err());
        let after = get_job_detail(&c, id).unwrap();
        assert_eq!(before.job.discovered_at, after.job.discovered_at);
        assert_eq!(before.history.len(), after.history.len());
        assert_eq!(dashboard_stats(&c, today()).unwrap().milestone_date, "2027-03-15");
    }

    #[test]
    fn dashboard_counts() {
        let c = open_in_memory().unwrap();
        let n = load_sample_data(&c).unwrap();
        assert!(n >= 8);
        let s = dashboard_stats(&c, today()).unwrap();
        assert_eq!(s.total_jobs, n as i64);
        assert!(s.submitted >= 2);
        assert!(s.interviews_scheduled >= 1);
        assert_eq!(s.activity.len(), 12);
        assert_eq!(s.unscored, n as i64, "Phase 1 never invents scores");
        assert!(s.interview_rate.is_some());
        assert!(s.excluded >= 1);
        let total_in_activity: i64 = s.activity.iter().map(|a| a.discovered).sum();
        assert_eq!(total_in_activity, n as i64);
        // Loading twice does not duplicate
        assert_eq!(load_sample_data(&c).unwrap(), 0);
        assert_eq!(remove_sample_data(&c).unwrap(), n);
        assert_eq!(dashboard_stats(&c, today()).unwrap().total_jobs, 0);
    }

    #[test]
    fn follow_up_detection() {
        let c = open_in_memory().unwrap();
        let id = insert_job(&c, &draft("Contoso", "Ops Analyst"), false, false).unwrap().id.unwrap();
        save_reminder(&c, &Reminder { id: None, job_id: id, due_date: today().to_string(), note: "Email recruiter".into(), done: false }).unwrap();
        let s = dashboard_stats(&c, today()).unwrap();
        assert_eq!(s.needs_follow_up, 1);
        assert_eq!(s.follow_ups[0].reason, "Email recruiter");
        assert!(save_reminder(&c, &Reminder { id: None, job_id: id, due_date: "soon".into(), note: String::new(), done: false }).is_err());
    }

    #[test]
    fn profile_crud_and_documents() {
        let c = open_in_memory().unwrap();
        save_profile_basics(&c, &ProfileBasics { full_name: "Sample Person".into(), ..Default::default() }).unwrap();
        let eid = save_experience(&c, &Experience { employer: "County EM".into(), title: "Coordinator".into(), is_current: true, ..Default::default() }).unwrap();
        save_skill(&c, &Skill { name: "Smartsheet".into(), ..Default::default() }).unwrap();
        save_skill(&c, &Skill { name: "smartsheet".into(), category: "Tools".into(), ..Default::default() }).unwrap();
        save_certification(&c, &Certification { name: "ITIL 4 Foundation".into(), ..Default::default() }).unwrap();
        save_accomplishment(&c, &Accomplishment { title: "Automated weekly report".into(), ..Default::default() }).unwrap();
        let d1 = insert_document(&c, "master_resume", "r.docx", "/x/r.docx", "abc", "text", None).unwrap();
        let d2 = insert_document(&c, "master_resume", "r2.docx", "/x/r2.docx", "def", "text", None).unwrap();
        assert_eq!((d1.version, d2.version), (1, 2));
        let p = get_profile(&c).unwrap();
        assert_eq!(p.basics.full_name, "Sample Person");
        assert_eq!(p.skills.len(), 1, "skills are unique case-insensitively");
        assert_eq!(p.skills[0].category, "Tools");
        assert_eq!(p.documents.len(), 2);
        delete_profile_item(&c, "experience", eid).unwrap();
        assert!(delete_profile_item(&c, "jobs; DROP TABLE jobs", 1).is_err());
        assert!(get_profile(&c).unwrap().experience.is_empty());
    }

    #[test]
    fn logs_are_searchable() {
        let c = open_in_memory().unwrap();
        log(&c, "error", "fetch", "Timeout contacting example.com");
        log(&c, "info", "import", "Saved job");
        assert_eq!(list_logs(&c, "timeout", 50).unwrap().len(), 1);
        assert_eq!(list_logs(&c, "", 50).unwrap().len(), 2);
    }
}
