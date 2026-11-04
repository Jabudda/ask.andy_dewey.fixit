//! CSV / Excel export and SQLite backup / restore.

use crate::db;
use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

pub const HEADERS: [&str; 22] = [
    "ID", "Employer", "Position", "Status", "Score", "Remote", "Location Eligibility", "Salary Min", "Salary Max",
    "Salary Period", "Salary Check", "Employment Type", "Posted", "Discovered", "Submitted", "Confirmation",
    "Posting URL", "Apply URL", "Interviews", "Excluded Reasons", "Notes", "Sample Data",
];

pub enum Cell {
    Text(String),
    Num(Option<f64>),
}

pub fn rows(c: &Connection) -> AppResult<Vec<Vec<Cell>>> {
    let jobs = db::list_jobs(c)?;
    let mut out = Vec::with_capacity(jobs.len());
    for j in jobs {
        let interviews: i64 = c.query_row("SELECT COUNT(*) FROM interviews WHERE job_id = ?1", [j.id], |r| r.get(0))?;
        let t = |s: &str| Cell::Text(s.to_string());
        let o = |s: &Option<String>| Cell::Text(s.clone().unwrap_or_default());
        out.push(vec![
            Cell::Num(Some(j.id as f64)),
            t(&j.draft.employer),
            t(&j.draft.title),
            t(&j.status),
            Cell::Num(j.score.map(|s| s as f64)),
            t(&j.draft.remote_type),
            t(&j.draft.wisconsin_eligibility),
            Cell::Num(j.draft.salary_min),
            Cell::Num(j.draft.salary_max),
            o(&j.draft.salary_period),
            t(&j.salary_flag),
            t(&j.draft.employment_type),
            o(&j.draft.posted_date),
            t(&j.discovered_at),
            o(&j.submitted_at),
            o(&j.confirmation_ref),
            o(&j.draft.posting_url),
            o(&j.draft.apply_url),
            Cell::Num(Some(interviews as f64)),
            t(&j.excluded_reasons.join("; ")),
            t(&j.notes),
            t(if j.is_sample { "yes" } else { "" }),
        ]);
    }
    Ok(out)
}

/// Prevent spreadsheet formula injection from text that originated on web pages.
pub fn csv_safe(s: &str) -> String {
    match s.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{s}"),
        _ => s.to_string(),
    }
}

pub fn export_csv(c: &Connection, path: &Path) -> AppResult<usize> {
    let data = rows(c)?;
    let mut w = csv::Writer::from_path(path).map_err(AppError::other)?;
    w.write_record(HEADERS).map_err(AppError::other)?;
    for r in &data {
        let rec: Vec<String> = r
            .iter()
            .map(|cell| match cell {
                Cell::Text(s) => csv_safe(s),
                Cell::Num(Some(n)) => {
                    if n.fract() == 0.0 { format!("{}", *n as i64) } else { n.to_string() }
                }
                Cell::Num(None) => String::new(),
            })
            .collect();
        w.write_record(&rec).map_err(AppError::other)?;
    }
    w.flush()?;
    Ok(data.len())
}

pub fn export_xlsx(c: &Connection, path: &Path) -> AppResult<usize> {
    use rust_xlsxwriter::{Format, Workbook};
    let data = rows(c)?;
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Applications").map_err(AppError::other)?;
    let bold = Format::new().set_bold();
    for (i, h) in HEADERS.iter().enumerate() {
        ws.write_string_with_format(0, i as u16, *h, &bold).map_err(AppError::other)?;
    }
    for (ri, r) in data.iter().enumerate() {
        let row = ri as u32 + 1;
        for (ci, cell) in r.iter().enumerate() {
            let col = ci as u16;
            match cell {
                Cell::Text(s) if !s.is_empty() => {
                    ws.write_string(row, col, s).map_err(AppError::other)?;
                }
                Cell::Num(Some(n)) => {
                    ws.write_number(row, col, *n).map_err(AppError::other)?;
                }
                _ => {}
            }
        }
    }
    ws.set_freeze_panes(1, 0).map_err(AppError::other)?;
    ws.autofilter(0, 0, data.len() as u32, HEADERS.len() as u16 - 1).map_err(AppError::other)?;
    wb.save(path).map_err(AppError::other)?;
    Ok(data.len())
}

/// Consistent online backup using SQLite's backup API (safe while the app is running).
pub fn backup(c: &Connection, dest: &Path) -> AppResult<()> {
    if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    c.backup(rusqlite::DatabaseName::Main, dest, None)?;
    Ok(())
}

/// Checks that a file is a Project Next Chapter database this version can read.
pub fn validate_backup(src: &Path) -> AppResult<i64> {
    let s = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| AppError::invalid("That file could not be opened as a database"))?;
    let tables: i64 = s
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('jobs','profile','settings','status_history')",
            [],
            |r| r.get(0),
        )
        .map_err(|_| AppError::invalid("That file is not a Project Next Chapter backup"))?;
    if tables != 4 {
        return Err(AppError::invalid("That file is not a Project Next Chapter backup"));
    }
    let v: i64 = s.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v > db::SCHEMA_VERSION {
        return Err(AppError::invalid("This backup was made by a newer version of the app"));
    }
    let integrity: String = s.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err(AppError::invalid(format!("Backup failed its integrity check: {integrity}")));
    }
    Ok(v)
}

/// Replaces the live database with a backup. A safety copy of the current data is written first.
pub fn restore(c: &mut Connection, src: &Path, safety_dir: &Path) -> AppResult<std::path::PathBuf> {
    validate_backup(src)?;
    std::fs::create_dir_all(safety_dir)?;
    let safety = safety_dir.join(format!("pre-restore-{}.sqlite", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    backup(c, &safety)?;
    c.restore(rusqlite::DatabaseName::Main, src, None::<fn(rusqlite::backup::Progress)>)?;
    db::migrate(c)?;
    db::log(c, "warn", "backup", &format!("Database restored from {}; previous data saved to {}", src.display(), safety.display()));
    Ok(safety)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::JobDraft;

    fn seed(c: &Connection) {
        let d = JobDraft {
            title: "Data Analyst".into(),
            employer: "=HYPERLINK(\"http://evil\")".into(),
            salary_min: Some(60000.0),
            salary_max: Some(70000.0),
            ..Default::default()
        };
        db::insert_job(c, &d, false, false).unwrap();
    }

    #[test]
    fn csv_and_xlsx_export() {
        let c = db::open_in_memory().unwrap();
        seed(&c);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("out.csv");
        assert_eq!(export_csv(&c, &p).unwrap(), 1);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("ID,Employer,Position"));
        assert!(text.contains("'=HYPERLINK"), "formula is neutralised: {text}");
        assert!(text.contains(",60000,70000,"));
        let x = dir.path().join("out.xlsx");
        assert_eq!(export_xlsx(&c, &x).unwrap(), 1);
        assert!(std::fs::metadata(&x).unwrap().len() > 1000);
    }

    #[test]
    fn backup_and_restore_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = db::open(&dir.path().join("live.sqlite")).unwrap();
        seed(&c);
        let b = dir.path().join("backup.sqlite");
        backup(&c, &b).unwrap();
        assert_eq!(validate_backup(&b).unwrap(), db::SCHEMA_VERSION);
        // Change live data, then restore
        c.execute("DELETE FROM jobs", []).unwrap();
        assert_eq!(db::list_jobs(&c).unwrap().len(), 0);
        let safety = restore(&mut c, &b, &dir.path().join("safety")).unwrap();
        assert!(safety.exists());
        assert_eq!(db::list_jobs(&c).unwrap().len(), 1);
    }

    #[test]
    fn rejects_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.sqlite");
        std::fs::write(&junk, b"hello").unwrap();
        assert!(validate_backup(&junk).is_err());
        let other = dir.path().join("other.sqlite");
        let o = Connection::open(&other).unwrap();
        o.execute_batch("CREATE TABLE foo (x)").unwrap();
        drop(o);
        assert!(validate_backup(&other).is_err());
    }
}
