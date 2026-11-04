//! Import of an existing job-search folder ("Command Center" layout):
//!
//! ```text
//! <root>/*Job_Search_Tracker*.xlsx        Tracker / Interviews sheets  → jobs, statuses, contacts, reminders
//! <root>/Profile_Links_Tracker*.xlsx      Profiles sheet               → LinkedIn URL / name (only if blank)
//! <root>/01 Resume*/                      .docx resumes                → master resume (versioned)
//! <root>/02 Applications Submitted/       sent resumes, cover letters, posting PDFs, confirmations → linked to jobs
//! <root>/1x *Certificates/                certificate PDFs             → certifications
//! <root>/07 Portfolio/, 08 Performance*/  work samples, reviews        → profile documents
//! ```
//!
//! Safe to re-run: jobs are matched by employer + title, documents by content hash. A dry run
//! executes the identical logic against an in-memory copy of the database and copies no files.
//! Files are never modified or moved; large media files are linked in place instead of copied.

use crate::db;
use crate::documents;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::parse;
use calamine::{open_workbook_auto, Data, Reader};
use chrono::{Duration, NaiveDate};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Files larger than this are referenced where they are instead of being copied.
pub const COPY_LIMIT: u64 = 25 * 1024 * 1024;

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub dry_run: bool,
    pub root: String,
    pub tracker_file: Option<String>,
    pub jobs: Vec<JobLine>,
    pub documents: Vec<DocLine>,
    pub certifications: Vec<String>,
    pub profile_updates: Vec<String>,
    pub suggested_skills: Vec<String>,
    pub warnings: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLine {
    pub job_id: i64,
    pub employer: String,
    pub title: String,
    pub status: String,
    pub applied: Option<String>,
    pub salary: String,
    /// "added" | "already in app"
    pub action: String,
    pub posting: Option<String>,
    pub linked_documents: usize,
    pub flags: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocLine {
    pub file: String,
    pub kind: String,
    pub job: Option<String>,
    /// "copied" | "linked in place" | "already imported"
    pub action: String,
    pub text_chars: usize,
    pub note: Option<String>,
}

pub fn run(live: &Connection, docs_dir: &Path, root: &Path, dry_run: bool) -> AppResult<ImportReport> {
    if !root.is_dir() {
        return Err(AppError::invalid(format!("{} is not a folder", root.display())));
    }
    if dry_run {
        let mut mem = Connection::open_in_memory()?;
        rusqlite::backup::Backup::new(live, &mut mem)?.run_to_completion(512, std::time::Duration::ZERO, None)?;
        mem.pragma_update(None, "foreign_keys", "ON")?;
        return import(&mem, docs_dir, root, true);
    }
    let report = import(live, docs_dir, root, false)?;
    db::log(
        live,
        "info",
        "import",
        &format!(
            "Imported folder {}: {} jobs added, {} documents added",
            root.display(),
            report.jobs.iter().filter(|j| j.action == "added").count(),
            report.documents.iter().filter(|d| d.action != "already imported").count()
        ),
    );
    Ok(report)
}

// ───────────────────────────── spreadsheet helpers ─────────────────────────────

fn cell_str(d: &Data) -> String {
    let s = match d {
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 { format!("{}", *f as i64) } else { f.to_string() }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt.as_f64().to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        _ => String::new(),
    };
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.eq_ignore_ascii_case("none") || s == "-" { String::new() } else { s }
}

fn serial_to_date(f: f64) -> Option<NaiveDate> {
    if !(20000.0..80000.0).contains(&f) {
        return None;
    }
    NaiveDate::from_ymd_opt(1899, 12, 30).map(|d| d + Duration::days(f.floor() as i64))
}

fn cell_date(d: &Data) -> Option<NaiveDate> {
    match d {
        Data::Float(f) => serial_to_date(*f),
        Data::Int(i) => serial_to_date(*i as f64),
        Data::DateTime(dt) => serial_to_date(dt.as_f64()),
        Data::DateTimeIso(s) | Data::String(s) => {
            let s = s.trim();
            s.parse::<f64>().ok().and_then(serial_to_date).or_else(|| {
                parse::normalize_date(s).and_then(|x| NaiveDate::parse_from_str(&x, "%Y-%m-%d").ok())
            })
        }
        _ => None,
    }
}

fn cell_num(d: &Data) -> Option<f64> {
    match d {
        Data::Float(f) => Some(*f),
        Data::Int(i) => Some(*i as f64),
        Data::String(s) => s.replace([',', '$'], "").trim().parse().ok(),
        _ => None,
    }
    .filter(|v| *v > 0.0)
}

fn read_sheet(path: &Path, name: &str) -> AppResult<Option<Vec<Vec<Data>>>> {
    let mut wb = open_workbook_auto(path).map_err(|e| AppError::invalid(format!("Could not read {}: {e}", path.display())))?;
    let Some(sheet) = wb.sheet_names().into_iter().find(|n| n.eq_ignore_ascii_case(name)) else { return Ok(None) };
    let range = wb.worksheet_range(&sheet).map_err(AppError::other)?;
    Ok(Some(range.rows().map(|r| r.to_vec()).collect()))
}

/// A sheet with a header row (found by required column names) as name→cell maps.
struct Table {
    cols: Vec<String>,
    rows: Vec<Vec<Data>>,
}

impl Table {
    fn from(rows: Vec<Vec<Data>>, required: &[&str]) -> Option<Table> {
        let hi = rows.iter().position(|r| {
            let names: Vec<String> = r.iter().map(|c| cell_str(c).to_lowercase()).collect();
            required.iter().all(|q| names.iter().any(|n| n == &q.to_lowercase()))
        })?;
        let cols = rows[hi].iter().map(|c| cell_str(c).to_lowercase()).collect();
        Some(Table { cols, rows: rows[hi + 1..].to_vec() })
    }
    fn idx(&self, name: &str) -> Option<usize> {
        let n = name.to_lowercase();
        self.cols.iter().position(|c| c == &n).or_else(|| self.cols.iter().position(|c| c.starts_with(&n)))
    }
    fn get<'a>(&self, row: &'a [Data], name: &str) -> Option<&'a Data> {
        self.idx(name).and_then(|i| row.get(i))
    }
    fn s(&self, row: &[Data], name: &str) -> String {
        self.get(row, name).map(cell_str).unwrap_or_default()
    }
    fn date(&self, row: &[Data], name: &str) -> Option<NaiveDate> {
        self.get(row, name).and_then(cell_date)
    }
    fn num(&self, row: &[Data], name: &str) -> Option<f64> {
        self.get(row, name).and_then(cell_num)
    }
}

// ───────────────────────────── matching helpers ─────────────────────────────

const STOP: &[&str] = &[
    "the", "and", "inc", "llc", "ltd", "corp", "corporation", "company", "group", "university", "of", "health",
    "services", "service", "labs", "lab", "solutions", "technologies", "technology", "systems", "global", "international",
    "california", "america", "american", "national", "data", "jobs", "careers",
];

fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(String::from)
        .collect()
}

fn employer_tokens(employer: &str) -> Vec<String> {
    tokens(employer).into_iter().filter(|t| !STOP.contains(&t.as_str())).collect()
}

struct JobRef {
    id: i64,
    employer: String,
    title: String,
    emp_tokens: Vec<String>,
    role_norm: String,
}

fn norm(s: &str) -> String {
    tokens(s).join(" ")
}

/// The single job whose distinctive employer words best match `hay`; None when ambiguous.
fn best_match<'a>(jobs: &'a [JobRef], hay_tokens: &[String]) -> Option<&'a JobRef> {
    let scored: Vec<(usize, &JobRef)> = jobs
        .iter()
        .map(|j| (j.emp_tokens.iter().filter(|t| hay_tokens.contains(t)).count(), j))
        .filter(|(n, _)| *n > 0)
        .collect();
    let top = scored.iter().map(|(n, _)| *n).max()?;
    let best: Vec<&JobRef> = scored.iter().filter(|(n, _)| *n == top).map(|(_, j)| *j).collect();
    (best.len() == 1).then(|| best[0])
}

/// Matches a file to one tracker row using, in order: its application folder name
/// (e.g. "14 Legence - HRIS Data Validation Analyst"), its file name, then the role title.
/// Document *text* is deliberately not used: postings and forms mention other employers.
fn match_job<'a>(jobs: &'a [JobRef], file_stem: &str, app_folder: Option<&str>) -> Option<&'a JobRef> {
    if let Some(f) = app_folder {
        if let Some(j) = best_match(jobs, &tokens(f)) {
            return Some(j);
        }
    }
    if let Some(j) = best_match(jobs, &tokens(file_stem)) {
        return Some(j);
    }
    let hay = format!("{} {}", norm(app_folder.unwrap_or("")), norm(file_stem));
    let by_role: Vec<&JobRef> = jobs.iter().filter(|j| !j.role_norm.is_empty() && hay.contains(&j.role_norm)).collect();
    (by_role.len() == 1).then(|| by_role[0])
}

static RE_CERT_DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(January|February|March|April|May|June|July|August|September|October|November|December|Jan|Feb|Mar|Apr|Jun|Jul|Aug|Sep|Sept|Oct|Nov|Dec)\.?\s+(\d{1,2}),?\s+(\d{4})\b|\b(\d{1,2})/(\d{1,2})/(\d{4})\b").unwrap()
});

static RE_CRED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(SMARTU[A-Za-z0-9]{10})").unwrap());

/// Certificate title: the text line that best matches the file name (fixes file-name typos).
fn cert_title(stem: &str, text: &str) -> String {
    let st = tokens(stem);
    text.lines()
        .map(str::trim)
        .filter(|l| l.len() >= 6 && l.len() <= 90)
        .map(|l| (tokens(l).iter().filter(|t| st.contains(t)).count(), l))
        .filter(|(n, _)| *n >= 2 || (*n >= 1 && st.len() == 1))
        .max_by_key(|(n, _)| *n)
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| stem.trim().to_string())
}

/// Earned month (YYYY-MM) when the certificate text contains exactly one date.
fn cert_month(text: &str) -> Option<String> {
    let mut found: Vec<String> = vec![];
    for c in RE_CERT_DATE.captures_iter(text) {
        let d = if let Some(m) = c.get(1) {
            NaiveDate::parse_from_str(&format!("{} {} {}", &m.as_str()[..3], &c[2], &c[3]), "%b %d %Y").ok()
        } else {
            NaiveDate::from_ymd_opt(c[6].parse().ok()?, c[4].parse().ok()?, c[5].parse().ok()?)
        };
        if let Some(d) = d {
            let s = d.format("%Y-%m").to_string();
            if !found.contains(&s) {
                found.push(s);
            }
        }
    }
    (found.len() == 1).then(|| found.remove(0))
}

// ───────────────────────────── file helpers ─────────────────────────────

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name.starts_with("~$") {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn ext(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}
fn stem(p: &Path) -> String {
    p.file_stem().and_then(|e| e.to_str()).unwrap_or("").to_string()
}
fn top_folder(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .ok()
        .and_then(|r| r.components().next())
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .unwrap_or_default()
}
/// The per-application folder directly under a top-level folder, if the file is inside one.
fn app_folder(root: &Path, p: &Path) -> Option<String> {
    let comps: Vec<String> = p.strip_prefix(root).ok()?.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    (comps.len() >= 3).then(|| comps[1].clone())
}
fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

struct Ctx<'a> {
    c: &'a Connection,
    root: &'a Path,
    docs_dir: PathBuf,
    dry_run: bool,
}

impl Ctx<'_> {
    /// Adds a document once (by content hash). Returns its extracted text when newly added.
    fn add_doc(&self, r: &mut ImportReport, path: &Path, kind: &str, job: Option<&JobRef>) -> AppResult<Option<String>> {
        let size = std::fs::metadata(path)?.len();
        let small = size <= COPY_LIMIT;
        let sha = if small {
            documents::sha256_hex(&std::fs::read(path)?)
        } else {
            documents::sha256_hex(format!("linked:{}:{size}", rel(self.root, path)).as_bytes())
        };
        let file = rel(self.root, path);
        let job_label = job.map(|j| format!("{} — {}", j.employer, j.title));
        let existing: Option<i64> = self.c.query_row("SELECT id FROM documents WHERE sha256 = ?1", [&sha], |x| x.get(0)).optional()?;
        if existing.is_some() {
            r.documents.push(DocLine { file, kind: kind.into(), job: job_label, action: "already imported".into(), text_chars: 0, note: None });
            return Ok(None);
        }
        let (text, note) = if small {
            match documents::extract_text(path) {
                Ok((t, w)) => (t, w.filter(|_| matches!(ext(path).as_str(), "pdf" | "docx"))),
                Err(e) => (String::new(), Some(e.to_string())),
            }
        } else {
            (String::new(), Some(format!("{} MB — linked where it is, not copied", size / 1_048_576)))
        };
        let stored = if small {
            let dest = self.docs_dir.join("command-center").join(path.strip_prefix(self.root).unwrap_or(path));
            if !self.dry_run {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(path, &dest)?;
            }
            dest
        } else {
            path.to_path_buf()
        };
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        db::insert_document(self.c, kind, &name, &stored.display().to_string(), &sha, &text, job.map(|j| j.id))?;
        r.documents.push(DocLine {
            file,
            kind: kind.into(),
            job: job_label,
            action: if small { "copied" } else { "linked in place" }.into(),
            text_chars: text.chars().count(),
            note,
        });
        Ok(Some(text))
    }
}

// ───────────────────────────── tracker rows ─────────────────────────────

/// Status path to walk for a tracker Stage. Starts after "Discovered".
fn status_path(stage: &str, phase: &str, applied: bool) -> Vec<&'static str> {
    let s = format!("{} {}", stage, phase).to_lowercase();
    let sub = |rest: &[&'static str]| -> Vec<&'static str> {
        let mut v = if applied { vec!["Submitted"] } else { vec![] };
        v.extend_from_slice(rest);
        v
    };
    if s.contains("offer") {
        sub(&["Interviewing", "Offer"])
    } else if s.contains("interview") {
        sub(&["Interviewing"])
    } else if s.contains("reject") {
        sub(&["Rejected"])
    } else if s.contains("withdr") {
        sub(&["Withdrawn"])
    } else if s.contains("closed") {
        sub(&["Closed"])
    } else if s.contains("applied") || s.contains("submitted") || applied {
        vec!["Submitted"]
    } else if s.contains("review") || s.contains("tailor") || s.contains("prep") {
        vec!["Under Review"]
    } else {
        vec![]
    }
}

fn at(d: NaiveDate, time: &str) -> String {
    format!("{d} {time}")
}

fn import_tracker(cx: &Ctx, r: &mut ImportReport, tracker: &Path) -> AppResult<Vec<JobRef>> {
    let mut refs = vec![];
    let Some(rows) = read_sheet(tracker, "Tracker")? else {
        r.warnings.push(format!("{} has no “Tracker” sheet", rel(cx.root, tracker)));
        return Ok(refs);
    };
    let Some(t) = Table::from(rows, &["Company", "Role"]) else {
        r.warnings.push("Could not find the Company / Role header row on the Tracker sheet".into());
        return Ok(refs);
    };
    let interviews = read_sheet(tracker, "Interviews")?.and_then(|rows| Table::from(rows, &["Date", "Company"]));

    for row in &t.rows {
        let employer = t.s(row, "Company");
        let title = t.s(row, "Role");
        if employer.is_empty() || title.is_empty() {
            continue;
        }
        let stage = t.s(row, "Stage");
        let phase = t.s(row, "Phase");
        let found = t.date(row, "Date Found");
        let applied = t.date(row, "Date Applied");
        let last_contact = t.date(row, "Last Contact");
        let location = t.s(row, "Location");
        let work = t.s(row, "Work Type").to_lowercase();
        let (lo, hi) = (t.num(row, "Salary Min"), t.num(row, "Salary Max"));
        let url = t.s(row, "Job Posting URL");

        let mut d = JobDraft {
            title: title.clone(),
            employer: employer.clone(),
            location_text: location.clone(),
            source: "command_center_tracker".into(),
            extraction_method: "manual".into(),
            ..Default::default()
        };
        d.detection_notes.push(format!("Imported from your tracker ({}, ID {})", rel(cx.root, tracker), t.s(row, "ID")));
        d.remote_type = if work.contains("hybrid") {
            "hybrid"
        } else if work.contains("site") || work.contains("office") {
            "onsite"
        } else if work.contains("remote") {
            "remote"
        } else {
            "unknown"
        }
        .into();
        if d.remote_type != "unknown" {
            d.detection_notes.push(format!("Work arrangement from your tracker (Work Type = {})", t.s(row, "Work Type")));
        }
        if lo.is_some() || hi.is_some() {
            d.salary_min = lo.or(hi);
            d.salary_max = hi.or(lo);
            let top = d.salary_max.unwrap_or(0.0);
            d.salary_period = Some(if top < 300.0 { "hour" } else { "year" }.into());
            d.detection_notes.push(format!("Pay from your tracker (read as per {})", d.salary_period.as_deref().unwrap_or("year")));
        }
        if url.starts_with("http") {
            d.posting_url = Some(url.clone());
            d.apply_url = Some(url);
        }
        d.posted_date = found.map(|x| x.to_string());
        d.employment_type = parse::detect_employment_type(&format!("{title} {location}"));
        parse::enrich(&mut d);

        let res = db::insert_job(cx.c, &d, false, false)?;
        let (id, action) = match (res.id, res.duplicate_of) {
            (Some(id), _) => (id, "added"),
            (None, Some(dup)) => (dup.id, "already in app"),
            _ => continue,
        };

        if action == "added" {
            for st in status_path(&stage, &phase, applied.is_some()) {
                db::change_status(
                    cx.c,
                    &StatusChange {
                        job_id: id,
                        to_status: st.into(),
                        note: format!("Imported from tracker (Stage: {stage})"),
                        confirmation_ref: None,
                        user_confirmed_submission: true,
                    },
                )?;
            }
            let disc = found.or(applied).map(|x| at(x, "09:00:00"));
            let sub = applied.map(|x| at(x, "12:00:00"));
            let last = last_contact.or(applied).or(found).map(|x| at(x, "17:00:00"));
            if let Some(dd) = &disc {
                cx.c.execute("UPDATE jobs SET discovered_at = ?1 WHERE id = ?2", params![dd, id])?;
                cx.c.execute("UPDATE status_history SET changed_at = ?1 WHERE job_id = ?2 AND from_status IS NULL", params![dd, id])?;
            }
            if let Some(sd) = &sub {
                cx.c.execute(
                    "UPDATE jobs SET submitted_at = ?1, confirmation_method = 'imported_tracker' WHERE id = ?2 AND submitted_at IS NOT NULL",
                    params![sd, id],
                )?;
                cx.c.execute("UPDATE status_history SET changed_at = ?1 WHERE job_id = ?2 AND to_status = 'Submitted'", params![sd, id])?;
            }
            if let Some(ld) = &last {
                cx.c.execute(
                    "UPDATE status_history SET changed_at = ?1 WHERE job_id = ?2 AND from_status IS NOT NULL AND to_status != 'Submitted'",
                    params![ld, id],
                )?;
                cx.c.execute("UPDATE jobs SET updated_at = ?1 WHERE id = ?2", params![ld, id])?;
            }

            let mut notes = vec![];
            for (label, col) in [
                ("Priority", "Priority"), ("Source", "Source"), ("Stage in tracker", "Stage"), ("Resume sent", "Resume Version Sent"),
                ("Cover letter", "Cover Letter?"), ("Got interview", "Got Interview?"), ("Next action", "Next Action"),
                ("Folder", "Command Center Folder"), ("Notes", "Notes"),
            ] {
                let v = t.s(row, col);
                if !v.is_empty() {
                    notes.push(format!("{label}: {v}"));
                }
            }
            db::update_notes(cx.c, id, &notes.join("\n"))?;

            let cname = t.s(row, "Contact Name");
            if !cname.is_empty() {
                let reach = t.s(row, "Contact Email");
                let (email, phone) = if reach.contains('@') { (reach, String::new()) } else { (String::new(), reach) };
                db::save_contact(cx.c, &Contact { id: None, job_id: id, name: cname, role: String::new(), email, phone, notes: "From tracker".into() })?;
            }
            let closed = matches!(status_path(&stage, &phase, applied.is_some()).last(), Some(&("Rejected" | "Withdrawn" | "Closed")));
            if let (Some(due), false) = (t.date(row, "Follow-Up Due"), closed) {
                let note = Some(t.s(row, "Next Action")).filter(|s| !s.is_empty()).unwrap_or_else(|| "Follow up (from tracker)".into());
                db::save_reminder(cx.c, &Reminder { id: None, job_id: id, due_date: due.to_string(), note, done: false })?;
            }
            if let Some(it) = &interviews {
                for irow in &it.rows {
                    let company = it.s(irow, "Company");
                    let Some(date) = it.date(irow, "Date") else { continue };
                    if company.is_empty() || parse::fingerprint(&company, "") != parse::fingerprint(&employer, "") {
                        continue;
                    }
                    let time = it.s(irow, "Time");
                    let when = if time.len() >= 4 && time.contains(':') { format!("{date}T{time}") } else { format!("{date}T09:00") };
                    let kind = [it.s(irow, "Round"), it.s(irow, "Format")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
                    let inotes = [it.s(irow, "Interviewer"), it.s(irow, "Notes")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" — ");
                    db::save_interview(cx.c, &Interview { id: None, job_id: id, scheduled_at: when, kind, notes: inotes, outcome: String::new() })?;
                }
            }
        }

        let job = db::get_job(cx.c, id)?;
        r.jobs.push(JobLine {
            job_id: id,
            employer: employer.clone(),
            title: title.clone(),
            status: job.status.clone(),
            applied: applied.map(|x| x.to_string()),
            salary: match (job.draft.salary_min, job.draft.salary_max) {
                (Some(a), Some(b)) if a != b => format!("{}–{} /{}", crate::eligibility::money(a), crate::eligibility::money(b), job.draft.salary_period.clone().unwrap_or_default()),
                (Some(a), _) | (_, Some(a)) => format!("{} /{}", crate::eligibility::money(a), job.draft.salary_period.clone().unwrap_or_default()),
                _ => "not listed".into(),
            },
            action: action.into(),
            posting: None,
            linked_documents: 0,
            flags: vec![],
        });
        refs.push(JobRef {
            id,
            emp_tokens: employer_tokens(&employer),
            role_norm: norm(&title),
            employer,
            title,
        });
    }
    Ok(refs)
}

const POSTING_NOTE: &str = "Description taken from your saved posting";

/// Uses a saved posting's text as the job description and re-checks location eligibility against it.
/// Only fills an empty description, or refreshes one that an earlier import took from a posting.
fn apply_posting(c: &Connection, job_id: i64, file: &str, text: &str, from_posting: bool) -> AppResult<bool> {
    let job = db::get_job(c, job_id)?;
    if (!job.draft.description.trim().is_empty() && !from_posting) || text.trim().len() < 200 {
        return Ok(false);
    }
    if from_posting && job.draft.description == text {
        return Ok(true);
    }
    let mut d = job.draft.clone();
    d.detection_notes.retain(|n| !n.starts_with(POSTING_NOTE) && !n.ends_with("(from saved posting)"));
    d.description = text.to_string();
    d.detection_notes.push(format!("{POSTING_NOTE} “{file}”"));
    let (wi, note) = parse::detect_wisconsin(text);
    if wi == "excluded" || d.wisconsin_eligibility == "unknown" {
        if wi != "unknown" {
            d.wisconsin_eligibility = wi;
            d.detection_notes.extend(note.map(|n| format!("{n} (from saved posting)")));
        }
    }
    if d.remote_type == "remote" {
        let (rt, note) = parse::detect_remote(text);
        if rt == "hybrid" || rt == "onsite" {
            d.detection_notes.push(format!(
                "Your tracker says Remote, but the saved posting suggests {rt}: {} — verify",
                note.unwrap_or_default()
            ));
        }
    }
    parse::enrich(&mut d);
    db::update_job(c, job_id, &d)?;
    Ok(true)
}

// ───────────────────────────── main import ─────────────────────────────

fn import(c: &Connection, docs_dir: &Path, root: &Path, dry_run: bool) -> AppResult<ImportReport> {
    let cx = Ctx { c, root, docs_dir: docs_dir.to_path_buf(), dry_run };
    let mut r = ImportReport { dry_run, root: root.display().to_string(), ..Default::default() };
    let mut files = vec![];
    walk(root, &mut files);

    // 1. Tracker (top-level workbook whose name mentions "tracker" but not "profile")
    let is_root = |p: &Path| p.parent() == Some(root);
    let mut trackers: Vec<&PathBuf> = files
        .iter()
        .filter(|p| is_root(p) && ext(p) == "xlsx")
        .filter(|p| {
            let n = stem(p).to_lowercase();
            n.contains("tracker") && !n.contains("profile")
        })
        .collect();
    trackers.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    let jobs = match trackers.last() {
        Some(t) => {
            r.tracker_file = Some(rel(root, t));
            if trackers.len() > 1 {
                r.warnings.push(format!("Several tracker workbooks found; used the most recently modified: {}", rel(root, t)));
            }
            import_tracker(&cx, &mut r, t)?
        }
        None => {
            r.warnings.push("No job tracker workbook (…Tracker….xlsx) found at the top of the folder".into());
            vec![]
        }
    };

    // 2. Profile links → LinkedIn URL / name, only where the profile is blank
    if let Some(links) = files.iter().find(|p| is_root(p) && ext(p) == "xlsx" && stem(p).to_lowercase().contains("profile_links")) {
        if let Some(t) = read_sheet(links, "Profiles")?.and_then(|rows| Table::from(rows, &["Platform"])) {
            let mut basics = db::get_profile(c)?.basics;
            let mut changed = false;
            for row in &t.rows {
                if !t.s(row, "Platform").eq_ignore_ascii_case("linkedin") {
                    continue;
                }
                let url = t.s(row, "Profile URL");
                if basics.linkedin_url.is_empty() && url.starts_with("http") {
                    basics.linkedin_url = url.clone();
                    r.profile_updates.push(format!("LinkedIn URL set to {url}"));
                    changed = true;
                }
                let name = t.s(row, "Username");
                if basics.full_name.is_empty() && !name.is_empty() && !name.starts_with('@') && !name.contains('/') {
                    basics.full_name = name.clone();
                    r.profile_updates.push(format!("Name set to {name} (from your LinkedIn row)"));
                    changed = true;
                }
            }
            if changed {
                db::save_profile_basics(c, &basics)?;
            }
        }
        r.skipped.push(format!("{} — used only for your LinkedIn link", rel(root, links)));
    }

    let mut master_text = String::new();
    let mut linked: std::collections::HashMap<i64, usize> = Default::default();
    let mut postings: std::collections::HashMap<i64, String> = Default::default();

    // 3. Resumes folder: .docx resumes become versioned master resumes (oldest first, so the newest is current)
    let mut masters: Vec<&PathBuf> = files
        .iter()
        .filter(|p| top_folder(root, p).contains("resume") && ext(p) == "docx" && stem(p).to_lowercase().contains("resume"))
        .collect();
    masters.sort_by_key(|p| (std::fs::metadata(p).and_then(|m| m.modified()).ok(), stem(p).to_lowercase().contains("v2")));
    for m in &masters {
        if let Some(t) = cx.add_doc(&mut r, m, "master_resume", None)? {
            master_text = t;
        }
    }
    if master_text.is_empty() {
        if let Some(m) = masters.last() {
            master_text = documents::extract_text(m).map(|x| x.0).unwrap_or_default();
        }
    }

    // 4. Everything else
    for p in &files {
        let top = top_folder(root, p);
        let name_l = stem(p).to_lowercase();
        let e = ext(p);
        if is_root(p) {
            if r.tracker_file.as_deref() != Some(&rel(root, p)) && !name_l.contains("profile_links") {
                r.skipped.push(format!("{} — top-level file", rel(root, p)));
            }
            continue;
        }
        if masters.contains(&p) {
            continue;
        }
        let in_template = app_folder(root, p).map(|f| f.to_lowercase().contains("copy this one")).unwrap_or(false);
        if top.contains("template") || top.contains("archive") || in_template {
            r.skipped.push(format!("{} — templates/archive folder", rel(root, p)));
            continue;
        }
        if top.contains("certificate") {
            let text = if ext(p) == "pdf" { documents::extract_text(p).map(|x| x.0).unwrap_or_default() } else { String::new() };
            let added = cx.add_doc(&mut r, p, "certification", None)?;
            let cname = cert_title(&stem(p), &text);
            let cred = RE_CRED.captures(&text).map(|m| m[1].to_string()).unwrap_or_default();
            let exists: bool = c.query_row(
                "SELECT COUNT(*) FROM certifications WHERE name = ?1 COLLATE NOCASE OR (?2 != '' AND credential_id = ?2)",
                params![cname, cred],
                |x| x.get::<_, i64>(0),
            )? > 0;
            if exists && added.is_some() {
                r.certifications.push(format!("{cname} — already recorded (duplicate file {})", rel(root, p)));
            }
            if !exists && added.is_some() {
                let issuer = if text.contains("Smartsheet University") {
                    "Smartsheet University"
                } else if format!("{top} {text}").to_lowercase().contains("smartsheet") {
                    "Smartsheet"
                } else {
                    ""
                };
                let month = cert_month(&text);
                db::save_certification(c, &Certification { id: None, name: cname.clone(), issuer: issuer.into(), date_earned: month.clone().unwrap_or_default(), expires: String::new(), credential_id: cred.clone() })?;
                let id_note = if cred.is_empty() { String::new() } else { format!(", credential {cred}") };
                r.certifications.push(match month {
                    Some(m) => format!("{cname} — earned {m}{id_note} (read from the certificate)"),
                    None => format!("{cname}{id_note} — earned date not found; add it in Career profile"),
                });
            }
            continue;
        }
        if top.contains("portfolio") || top.contains("performance") {
            let kind = if top.contains("performance") { "performance_review" } else { "portfolio" };
            cx.add_doc(&mut r, p, kind, None)?;
            continue;
        }
        if top.contains("resume") && e == "pdf" && name_l.contains("resume") && !name_l.contains("cover") {
            r.skipped.push(format!("{} — PDF copy of a .docx resume", rel(root, p)));
            continue;
        }

        // Application materials (02 Applications Submitted, 03–06 pipeline folders, leftovers in 01)
        // Documents you sent: any file named like a resume, cover letter or application package.
        let flat = name_l.replace([' ', '-'], "_");
        let sent = flat.contains("resume") || flat.contains("cover_letter") || flat.contains("application_package");
        let kind = if sent {
            if name_l.contains("cover") { "sent_cover_letter" } else if name_l.contains("resume") { "sent_resume" } else { "application_package" }
        } else if matches!(e.as_str(), "png" | "jpg" | "jpeg" | "heic")
            || name_l.contains("completed")
            || name_l.contains("confirmation")
            || name_l.contains("thank you for applying")
        {
            "application_confirmation"
        } else if e == "pdf" {
            "job_posting"
        } else {
            "career_document"
        };
        let job = match_job(&jobs, &stem(p), app_folder(root, p).as_deref());
        if job.is_none() && (kind == "job_posting" || sent) {
            r.warnings.push(format!("{} — could not match it to a single tracker row; stored unlinked", rel(root, p)));
        }
        let added = cx.add_doc(&mut r, p, kind, job)?;
        if let (Some(j), Some(_)) = (job, added) {
            *linked.entry(j.id).or_default() += 1;
        }
    }

    // 4b. Job descriptions from saved posting PDFs. A "posting" that contains your own name is an
    //     application page showing your resume, so it is never used — it would credit the posting
    //     with your own skills.
    let me = db::get_profile(c)?.basics.full_name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    for j in &jobs {
        let docs = db::list_documents(c, Some(j.id))?;
        let current = db::get_job(c, j.id)?;
        let from_posting = current.draft.detection_notes.iter().any(|n| n.starts_with(POSTING_NOTE))
            || (!current.draft.description.is_empty()
                && docs.iter().any(|d| d.kind == "job_posting" && (d.extracted_text == current.draft.description || parse::unwrap_text(&d.extracted_text) == current.draft.description)));
        let mut cands: Vec<&DocumentRec> = docs
            .iter()
            .filter(|d| d.kind == "job_posting" && d.extracted_text.trim().len() >= 200)
            .filter(|d| me.len() < 4 || !d.extracted_text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase().contains(&me))
            .collect();
        cands.sort_by_key(|d| std::cmp::Reverse(d.extracted_text.len()));
        if let Some(d) = cands.first() {
            if apply_posting(c, j.id, &d.file_name, &parse::unwrap_text(&d.extracted_text), from_posting)? {
                postings.insert(j.id, d.file_name.clone());
            }
        } else {
            // An earlier import may have used a page that turned out to be your own application.
            let job = current;
            if from_posting {
                let mut d = job.draft.clone();
                d.detection_notes.retain(|n| !n.starts_with(POSTING_NOTE) && !n.ends_with("(from saved posting)"));
                d.description.clear();
                db::update_job(c, j.id, &d)?;
                r.warnings.push(format!("{} — {}: the saved “posting” is your own application page, so it is no longer used as the description", j.employer, j.title));
            }
        }
    }

    // 5. Skill suggestions from the current master resume (never added automatically)
    let have: Vec<String> = db::get_profile(c)?.skills.into_iter().map(|s| s.name.to_lowercase()).collect();
    r.suggested_skills = documents::suggest_skills(&master_text).into_iter().filter(|s| !have.contains(&s.to_lowercase())).collect();

    // 6. Final flags per job (after postings may have changed eligibility)
    for line in r.jobs.iter_mut() {
        let j = db::get_job(c, line.job_id)?;
        line.flags = j.excluded_reasons.iter().map(|x| format!("⛔ {x}")).chain(j.review_flags.iter().map(|x| format!("⚠️ {x}"))).collect();
        line.posting = postings.get(&line.job_id).cloned();
        line.linked_documents = linked.get(&line.job_id).copied().unwrap_or(0);
    }
    if let Some(t) = &r.tracker_file {
        if let Some(rows) = read_sheet(&root.join(t), "Activity Log")? {
            let n = rows.iter().filter(|r| r.iter().any(|c| cell_date(c).is_some())).count();
            if n > 0 {
                r.skipped.push(format!("Activity Log sheet ({n} entries) — stays in your spreadsheet; status history is rebuilt from the Tracker sheet"));
            }
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::test_support::{make_docx, make_pdf};
    use rust_xlsxwriter::Workbook;

    fn write_tracker(path: &Path) {
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        ws.set_name("Tracker").unwrap();
        ws.write_string(0, 0, "Job Search Tracker").unwrap();
        let hdr = [
            "ID", "Priority", "Company", "Role", "Stage", "Phase", "Date Found", "Date Applied", "Last Contact", "Follow-Up Due",
            "Next Action", "Work Type", "Location", "Salary Min", "Salary Max", "Source", "Contact Name", "Contact Email / Phone",
            "Resume Version Sent", "Job Posting URL", "Notes",
        ];
        for (i, h) in hdr.iter().enumerate() {
            ws.write_string(2, i as u16, *h).unwrap();
        }
        // 46301 = 2026-10-06
        let rows: [[&str; 21]; 3] = [
            ["1", "Low", "Acme Analytics", "Data Analyst", "Applied", "Applied", "46301", "46302", "46302", "46320", "Email recruiter",
             "Remote", "Remote (United States)", "65000", "80000", "Indeed", "Pat Recruiter", "pat@acme.example", "Jordan_Sample_Acme_Resume", "https://jobs.acme.example/1", "Strong fit"],
            ["2", "High", "Globex", "IT Support Specialist", "Closed - Rejected (Their Loss)", "Closed", "46301", "46301", "46310", "None",
             "None", "Remote", "Remote", "30", "35", "LinkedIn", "", "", "", "", ""],
            ["3", "High", "Initech", "Project Coordinator", "Target - Apply ASAP", "Target", "46305", "", "", "", "Tailor resume",
             "Hybrid", "Chicago, IL", "", "", "Company Site", "", "", "", "", ""],
        ];
        for (ri, row) in rows.iter().enumerate() {
            for (ci, v) in row.iter().enumerate() {
                let (r, c) = (ri as u32 + 3, ci as u16);
                match v.parse::<f64>() {
                    Ok(n) if ci != 0 => { ws.write_number(r, c, n).unwrap(); }
                    _ if !v.is_empty() => { ws.write_string(r, c, *v).unwrap(); }
                    _ => {}
                }
            }
        }
        wb.save(path).unwrap();
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for f in ["01 Resume Original", "02 Applications Submitted", "07 Portfolio", "09 Job Tracker Excel Templates", "10 SmartSheet Certificates"] {
            std::fs::create_dir_all(root.join(f)).unwrap();
        }
        write_tracker(&root.join("10112026Job_Search_Tracker_Original.xlsx"));
        std::fs::write(root.join("01 Resume Original/Jordan_Sample_Master_Resume.docx"), make_docx(&["Built Smartsheet dashboards and Excel Office Scripts; led UAT"])).unwrap();
        std::fs::write(root.join("02 Applications Submitted/Jordan_Sample_Acme_Data_Analyst_Resume.pdf"), make_pdf(&["Tailored resume for Acme"])).unwrap();
        std::fs::write(root.join("02 Applications Submitted/Jordan_Sample_Globex_Cover_Letter.pdf"), make_pdf(&["Dear Globex"])).unwrap();
        let long = "Data Analyst at Acme Analytics. Fully remote. We can only hire in the following states: Texas, Florida, Georgia. Responsibilities include reporting, dashboards, SQL, Excel and stakeholder communication across many teams and departments.";
        std::fs::write(root.join("02 Applications Submitted/Data Analyst - Posting.pdf"), make_pdf(&[&long[..90], &long[90..180], &long[180..]])).unwrap();
        std::fs::write(root.join("02 Applications Submitted/Confirmation Number.png"), b"\x89PNG").unwrap();
        std::fs::write(root.join("07 Portfolio/Inbound Call Template.docx"), make_docx(&["Call template"])).unwrap();
        std::fs::write(root.join("09 Job Tracker Excel Templates/template.xlsx"), b"x").unwrap();
        std::fs::write(root.join("10 SmartSheet Certificates/Builder Certificate.pdf"), make_pdf(&["Jordan Sample", "Builder Certificate of Achievement", "Smartsheet University", "Completed: SMARTUk01a9765vvApril 21, 2025"])).unwrap();
        std::fs::write(root.join("10 SmartSheet Certificates/Builder Certificate of Achievment.pdf"), make_pdf(&["Jordan Sample", "Builder Certificate of Achievement", "Smartsheet University", "Completed: SMARTUk01a9765vvApril 21, 2025", "copy"])).unwrap();
        // Per-application folder: a confirmation whose file name has no employer in it
        std::fs::create_dir_all(root.join("02 Applications Submitted/02 Globex - IT Support/04 Application Responses")).unwrap();
        std::fs::write(root.join("02 Applications Submitted/02 Globex - IT Support/04 Application Responses/Confirmation Number.png"), b"\x89PNG2").unwrap();
        // A loose form that mentions an employer in its text must not be matched by text
        std::fs::write(root.join("02 Applications Submitted/Office Application Manager Plus.pdf"), make_pdf(&["Senate Resume Bank", "Previous application: Acme Analytics Data Analyst"])).unwrap();
        std::fs::create_dir_all(root.join("02 Applications Submitted/100 Company Name - Copy This One/01 Job Posting")).unwrap();
        std::fs::write(root.join("02 Applications Submitted/100 Company Name - Copy This One/01 Job Posting/placeholder.pdf"), make_pdf(&["template"])).unwrap();
        dir
    }

    #[test]
    fn dry_run_changes_nothing() {
        let dir = fixture();
        let docs = tempfile::tempdir().unwrap();
        let c = db::open_in_memory().unwrap();
        let r = run(&c, docs.path(), dir.path(), true).unwrap();
        assert_eq!(r.jobs.len(), 3);
        assert!(r.jobs.iter().all(|j| j.action == "added"));
        assert!(!r.documents.is_empty());
        assert_eq!(db::list_jobs(&c).unwrap().len(), 0, "dry run must not write to the real database");
        assert_eq!(db::get_profile(&c).unwrap().documents.len(), 0);
        assert!(std::fs::read_dir(docs.path()).unwrap().next().is_none(), "dry run must not copy files");
    }

    #[test]
    fn imports_and_is_idempotent() {
        let dir = fixture();
        let docs = tempfile::tempdir().unwrap();
        let c = db::open_in_memory().unwrap();
        let r = run(&c, docs.path(), dir.path(), false).unwrap();

        let jobs = db::list_jobs(&c).unwrap();
        assert_eq!(jobs.len(), 3);
        let acme = jobs.iter().find(|j| j.draft.employer == "Acme Analytics").unwrap();
        assert_eq!(acme.status, "Submitted");
        assert_eq!(acme.submitted_at.as_deref(), Some("2026-10-07 12:00:00"));
        assert_eq!(acme.confirmation_method.as_deref(), Some("imported_tracker"));
        assert_eq!(acme.discovered_at, "2026-10-06 09:00:00");
        assert!(acme.draft.description.contains("Responsibilities"), "posting PDF became the description");
        assert_eq!(acme.draft.wisconsin_eligibility, "excluded", "posting's state list overrides tracker's 'United States'");
        assert!(acme.notes.contains("Source: Indeed"));
        let detail = db::get_job_detail(&c, acme.id).unwrap();
        assert_eq!(detail.contacts[0].email, "pat@acme.example");
        assert_eq!(detail.reminders.len(), 1);
        assert!(detail.documents.iter().any(|d| d.kind == "sent_resume"));
        assert!(detail.documents.iter().any(|d| d.kind == "job_posting"));

        let globex = jobs.iter().find(|j| j.draft.employer == "Globex").unwrap();
        assert_eq!(globex.status, "Rejected");
        assert_eq!(globex.draft.salary_period.as_deref(), Some("hour"));
        let gh = db::get_job_detail(&c, globex.id).unwrap();
        assert_eq!(gh.history.iter().map(|h| h.to_status.as_str()).collect::<Vec<_>>(), ["Rejected", "Submitted", "Discovered"]);
        assert!(gh.reminders.is_empty(), "no reminders for closed jobs");
        assert!(gh.documents.iter().any(|d| d.kind == "sent_cover_letter"));

        let initech = jobs.iter().find(|j| j.draft.employer == "Initech").unwrap();
        assert_eq!(initech.status, "Discovered");
        assert!(initech.submitted_at.is_none());
        assert!(!initech.excluded_reasons.is_empty(), "hybrid is excluded");

        let p = db::get_profile(&c).unwrap();
        assert!(p.documents.iter().any(|d| d.kind == "master_resume"));
        assert!(p.documents.iter().any(|d| d.kind == "portfolio"));
        assert_eq!(p.certifications.len(), 1);
        assert_eq!(p.certifications[0].name, "Builder Certificate of Achievement", "title from the certificate, not the misspelled file name");
        assert_eq!(p.certifications[0].date_earned, "2025-04");
        assert_eq!(p.certifications[0].credential_id, "SMARTUk01a9765vv");
        assert_eq!(p.certifications[0].issuer, "Smartsheet University");
        assert!(gh.documents.iter().any(|d| d.kind == "application_confirmation"), "linked through its application folder");
        let loose = p.documents.iter().find(|d| d.file_name == "Office Application Manager Plus.pdf").expect("stored");
        assert_eq!(loose.job_id, None, "never matched by document text");
        assert!(r.skipped.iter().any(|s| s.contains("Copy This One")));
        assert!(p.skills.is_empty(), "skills are suggested, never added");
        assert!(r.suggested_skills.contains(&"Smartsheet".to_string()));
        assert!(r.skipped.iter().any(|s| s.contains("templates")));
        assert!(r.documents.iter().any(|d| d.file.ends_with("Confirmation Number.png")));
        assert!(r.certifications.iter().any(|x| x.contains("already recorded")), "duplicate certificate file detected");
        // files were copied into the app's folder, originals untouched
        assert!(docs.path().join("command-center/02 Applications Submitted/Data Analyst - Posting.pdf").exists());
        assert!(dir.path().join("02 Applications Submitted/Data Analyst - Posting.pdf").exists());

        // Second run: nothing new
        let r2 = run(&c, docs.path(), dir.path(), false).unwrap();
        assert!(r2.jobs.iter().all(|j| j.action == "already in app"));
        assert!(r2.documents.iter().all(|d| d.action == "already imported"), "{:?}", r2.documents);
        assert_eq!(db::list_jobs(&c).unwrap().len(), 3);
        assert_eq!(db::get_profile(&c).unwrap().certifications.len(), 1);
    }

    #[test]
    fn own_application_page_is_not_used_as_description() {
        let dir = fixture();
        let root = dir.path();
        std::fs::remove_file(root.join("02 Applications Submitted/Data Analyst - Posting.pdf")).unwrap();
        let page = "Data Analyst at Acme Analytics application review page showing the candidate details below and the resume they uploaded for this role today";
        std::fs::write(root.join("02 Applications Submitted/Data Analyst - Posting.pdf"), make_pdf(&[&page[..70], &page[70..], "Applicant: Jordan", "Sample", "Crisis Management experience and many other skills listed here"])).unwrap();
        let docs = tempfile::tempdir().unwrap();
        let c = db::open_in_memory().unwrap();
        db::save_profile_basics(&c, &ProfileBasics { full_name: "Jordan Sample".into(), ..Default::default() }).unwrap();
        run(&c, docs.path(), root, false).unwrap();
        let acme = db::list_jobs(&c).unwrap().into_iter().find(|j| j.draft.employer == "Acme Analytics").unwrap();
        assert!(acme.draft.description.is_empty(), "application page with the applicant's name must not become the description");
    }

    #[test]
    fn helpers() {
        assert_eq!(serial_to_date(46301.0), NaiveDate::from_ymd_opt(2026, 10, 6));
        assert_eq!(cell_str(&Data::String("None".into())), "");
        assert_eq!(cell_str(&Data::String("University of California\nSan Francisco".into())), "University of California San Francisco");
        assert_eq!(status_path("Applied", "Applied", true), vec!["Submitted"]);
        assert_eq!(status_path("Closed - Rejected (Their Loss)", "Closed", true), vec!["Submitted", "Rejected"]);
        assert!(status_path("Target - Apply ASAP", "Target", false).is_empty());
        assert_eq!(cert_month("Issued on April 14, 2025"), Some("2025-04".into()));
        assert_eq!(cert_month("Completed: SMARTUk01a9765vvApril 21, 2025"), Some("2025-04".into()));
        assert_eq!(cert_title("Builder Certificate of Achievment", "Jordan Sample\nBuilder Certificate of Achievement\nSmartsheet University"), "Builder Certificate of Achievement");
        assert_eq!(cert_month("Apr 1, 2025 and May 2, 2025"), None);
        assert_eq!(employer_tokens("University of California San Francisco (UCSF)"), vec!["san", "francisco", "ucsf"]);
    }
}
