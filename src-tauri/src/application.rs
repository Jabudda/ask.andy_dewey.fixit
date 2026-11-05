//! Phase 4: application assistance.
//!
//! The review queue, the application packet (job + documents + answers), proposed answers to common
//! application questions, and the approval gate. Nothing here submits anything: the app opens the
//! official application page and the candidate applies there.
//!
//! Truthfulness: answers come only from the candidate's own standard answers, their profile, their
//! preferences or the saved posting. When none of those cover a question, it is marked "needs your
//! answer" and the job cannot be approved until the candidate writes one.

use crate::db::{self, now};
use crate::error::{AppError, AppResult};
use crate::models::{DocumentRec, Job, StatusChange};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Statuses that belong in the review queue, in workflow order.
pub const QUEUE_STATUSES: [&str; 4] = ["Documents Ready", "Awaiting Approval", "Approved", "In Progress"];

/// Standard questions: (key, question, hint shown in the answer bank).
pub const STANDARD: [(&str, &str, &str); 9] = [
    ("work_authorized", "Are you legally authorized to work in the United States?", "e.g. Yes"),
    ("sponsorship", "Will you now or in the future require visa sponsorship?", "e.g. No"),
    ("start_date", "When are you available to start?", "e.g. Two weeks after an offer"),
    ("salary", "What are your salary expectations?", "Leave blank to use the posted range or your minimum"),
    ("location", "Where are you located?", "Taken from your profile"),
    ("linkedin", "LinkedIn profile URL", "Taken from your profile"),
    ("travel", "Are you able to travel as required for this role?", "Only asked when the posting mentions travel"),
    ("heard_about", "How did you hear about this position?", "Filled in from where the job was found"),
    ("why_interested", "Why are you interested in this role?", "Always written by you for each job"),
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BankAnswer {
    pub key: String,
    pub question: String,
    pub hint: String,
    pub answer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct JobAnswer {
    pub id: Option<i64>,
    pub job_id: i64,
    pub key: String,
    pub question: String,
    pub answer: String,
    pub source: String,
    /// proposed | needs_input | confirmed
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Packet {
    pub job: Job,
    pub resume: Option<DocumentRec>,
    pub cover_letter: Option<DocumentRec>,
    pub answers: Vec<JobAnswer>,
    /// Reasons the job cannot be approved yet (empty = ready).
    pub blockers: Vec<String>,
    pub follow_up_days: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub job: Job,
    pub resume_approved: bool,
    pub cover_letter_approved: bool,
    pub answers_needing_input: usize,
    pub blockers: Vec<String>,
}

// ───────────────────────────── answer bank ─────────────────────────────

pub fn answer_bank(c: &Connection) -> AppResult<Vec<BankAnswer>> {
    let mut out = vec![];
    for (key, question, hint) in STANDARD {
        let answer: Option<String> = c.query_row("SELECT answer FROM answer_bank WHERE key = ?1", [key], |r| r.get(0)).optional()?;
        out.push(BankAnswer { key: key.into(), question: question.into(), hint: hint.into(), answer: answer.unwrap_or_default() });
    }
    Ok(out)
}

pub fn save_bank_answer(c: &Connection, key: &str, answer: &str) -> AppResult<()> {
    if !STANDARD.iter().any(|(k, ..)| *k == key) {
        return Err(AppError::invalid(format!("Unknown standard question “{key}”")));
    }
    c.execute(
        "INSERT INTO answer_bank (key, answer, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET answer = excluded.answer, updated_at = excluded.updated_at",
        params![key, answer.trim(), now()],
    )?;
    Ok(())
}

fn bank(c: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(c
        .query_row("SELECT answer FROM answer_bank WHERE key = ?1", [key], |r| r.get::<_, String>(0))
        .optional()?
        .filter(|s| !s.trim().is_empty()))
}

// ───────────────────────────── proposals ─────────────────────────────

fn mentions_travel(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("travel") && !t.contains("no travel")
}

fn heard_about(job: &Job) -> String {
    if job.draft.internal_candidate {
        return "Internal posting (I am a current employee)".into();
    }
    match job.draft.source.as_str() {
        "usajobs" => "USAJOBS".into(),
        "greenhouse" | "lever" | "ashby" => format!("{} careers page", job.draft.employer),
        "remotive" => "Remotive job board".into(),
        "himalayas" => "Himalayas job board".into(),
        "jobicy" => "Jobicy job board".into(),
        _ => String::new(),
    }
}

/// One proposal: (answer, source, status). Empty answer means the candidate must write it.
fn propose(c: &Connection, key: &str, job: &Job) -> AppResult<(String, String, &'static str)> {
    let from_bank = |c: &Connection| -> AppResult<Option<(String, String, &'static str)>> {
        Ok(bank(c, key)?.map(|a| (a, "Your standard answer".to_string(), "proposed")))
    };
    let need = || Ok((String::new(), "No source covers this. Please write it.".to_string(), "needs_input"));
    match key {
        "work_authorized" | "sponsorship" | "start_date" | "travel" => from_bank(c)?.map_or_else(need, Ok),
        "salary" => {
            if let Some(x) = from_bank(c)? {
                return Ok(x);
            }
            let money = crate::eligibility::money;
            match (job.draft.salary_min, job.draft.salary_max, job.draft.salary_period.as_deref()) {
                (Some(lo), Some(hi), Some("year") | None) => Ok((
                    format!("I am open to the posted range of {} to {}.", money(lo), money(hi)),
                    "The posting's pay range (meets your minimum in Preferences)".into(),
                    "proposed",
                )),
                _ => {
                    let p = db::get_preferences(c)?;
                    match Some(p.min_annual_salary) {
                        Some(m) if m > 0.0 => Ok((
                            format!("{} or more per year, depending on the full role and benefits.", money(m)),
                            "Your minimum salary in Preferences".into(),
                            "proposed",
                        )),
                        _ => need(),
                    }
                }
            }
        }
        "location" => {
            let p = db::get_profile(c)?;
            let l = p.basics.location.trim().to_string();
            if l.is_empty() { need() } else { Ok((l, "Your career profile".into(), "proposed")) }
        }
        "linkedin" => {
            let p = db::get_profile(c)?;
            let l = p.basics.linkedin_url.trim().to_string();
            if l.is_empty() { need() } else { Ok((l, "Your career profile".into(), "proposed")) }
        }
        "heard_about" => {
            let h = heard_about(job);
            if h.is_empty() { need() } else { Ok((h, "Where this job was found".into(), "proposed")) }
        }
        _ => need(),
    }
}

/// Fills in proposals for the standard questions. Answers you confirmed or edited are never overwritten.
pub fn refresh_proposals(c: &Connection, job_id: i64) -> AppResult<Vec<JobAnswer>> {
    let job = db::get_job(c, job_id)?;
    for (i, (key, question, _)) in STANDARD.iter().enumerate() {
        if *key == "travel" && !mentions_travel(&job.draft.description) {
            continue;
        }
        let existing: Option<String> = c
            .query_row("SELECT status FROM job_answers WHERE job_id = ?1 AND key = ?2", params![job_id, key], |r| r.get(0))
            .optional()?;
        if existing.as_deref() == Some("confirmed") {
            continue;
        }
        let (answer, source, status) = propose(c, key, &job)?;
        c.execute(
            "INSERT INTO job_answers (job_id, key, question, answer, source, status, sort_order, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(job_id, question) DO UPDATE SET answer = excluded.answer, source = excluded.source,
                status = excluded.status, updated_at = excluded.updated_at",
            params![job_id, key, question, answer, source, status, i as i64, now()],
        )?;
    }
    answers(c, job_id)
}

pub fn answers(c: &Connection, job_id: i64) -> AppResult<Vec<JobAnswer>> {
    let mut st = c.prepare(
        "SELECT id, job_id, key, question, answer, source, status FROM job_answers WHERE job_id = ?1 ORDER BY sort_order, id",
    )?;
    let rows = st.query_map([job_id], |r| {
        Ok(JobAnswer {
            id: r.get(0)?,
            job_id: r.get(1)?,
            key: r.get(2)?,
            question: r.get(3)?,
            answer: r.get(4)?,
            source: r.get(5)?,
            status: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Saves your edit (or your own extra question). Anything you save counts as confirmed.
pub fn save_answer(c: &Connection, a: &JobAnswer) -> AppResult<i64> {
    let q = a.question.trim();
    if q.is_empty() {
        return Err(AppError::invalid("Write the question first"));
    }
    let answer = a.answer.trim();
    let status = if answer.is_empty() { "needs_input" } else { "confirmed" };
    match a.id {
        Some(id) => {
            c.execute(
                "UPDATE job_answers SET question = ?1, answer = ?2, source = 'You', status = ?3, updated_at = ?4 WHERE id = ?5 AND job_id = ?6",
                params![q, answer, status, now(), id, a.job_id],
            )?;
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO job_answers (job_id, key, question, answer, source, status, sort_order, updated_at)
                 VALUES (?1, '', ?2, ?3, 'You', ?4, 100, ?5)",
                params![a.job_id, q, answer, status, now()],
            )?;
            Ok(c.last_insert_rowid())
        }
    }
}

pub fn delete_answer(c: &Connection, id: i64) -> AppResult<()> {
    let key: Option<String> = c.query_row("SELECT key FROM job_answers WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    match key.as_deref() {
        None => Ok(()),
        Some("") => {
            c.execute("DELETE FROM job_answers WHERE id = ?1", [id])?;
            Ok(())
        }
        Some(_) => Err(AppError::invalid("Standard questions can't be removed; edit the answer instead")),
    }
}

// ───────────────────────────── packet & gate ─────────────────────────────

fn latest_doc(c: &Connection, job_id: i64, kind: &str) -> AppResult<Option<DocumentRec>> {
    Ok(db::list_documents(c, Some(job_id))?
        .into_iter()
        .filter(|d| d.kind == kind)
        .max_by_key(|d| (d.version, d.id)))
}

fn approved(d: &Option<DocumentRec>) -> bool {
    d.as_ref().is_some_and(|d| d.status == "approved")
}

/// Why this job can't be approved yet. Sample jobs are exempt (they have no documents).
pub fn blockers(c: &Connection, job: &Job) -> AppResult<Vec<String>> {
    if job.is_sample {
        return Ok(vec![]);
    }
    let mut out = vec![];
    let resume = latest_doc(c, job.id, "generated_resume")?;
    if !approved(&resume) {
        out.push(if resume.is_some() { "Approve the latest tailored resume" } else { "Generate and approve a tailored resume" }.to_string());
    }
    let cl = latest_doc(c, job.id, "generated_cover_letter")?;
    if cl.is_some() && !approved(&cl) {
        out.push("Approve the latest cover letter, or delete it".into());
    }
    let need = answers(c, job.id)?.iter().filter(|a| a.status == "needs_input").count();
    if need > 0 {
        out.push(format!("Answer {need} application question{} that only you can answer", if need == 1 { "" } else { "s" }));
    }
    if !job.excluded_reasons.is_empty() {
        out.push(format!("This job fails one of your rules: {}", job.excluded_reasons.join("; ")));
    }
    Ok(out)
}

pub fn follow_up_days(c: &Connection) -> AppResult<i64> {
    Ok(db::get_setting(c, "follow_up_days")?.and_then(|s| s.parse().ok()).filter(|d: &i64| (1..=60).contains(d)).unwrap_or(7))
}

pub fn set_follow_up_days(c: &Connection, days: i64) -> AppResult<()> {
    if !(1..=60).contains(&days) {
        return Err(AppError::invalid("Choose between 1 and 60 days"));
    }
    db::set_setting(c, "follow_up_days", &days.to_string())
}

pub fn packet(c: &Connection, job_id: i64) -> AppResult<Packet> {
    let job = db::get_job(c, job_id)?;
    let answers = if job.status == "Submitted" || !QUEUE_STATUSES.contains(&job.status.as_str()) {
        answers(c, job_id)?
    } else {
        refresh_proposals(c, job_id)?
    };
    Ok(Packet {
        resume: latest_doc(c, job_id, "generated_resume")?,
        cover_letter: latest_doc(c, job_id, "generated_cover_letter")?,
        blockers: blockers(c, &job)?,
        follow_up_days: follow_up_days(c)?,
        answers,
        job,
    })
}

pub fn queue(c: &Connection) -> AppResult<Vec<QueueItem>> {
    let mut out = vec![];
    for job in db::list_jobs(c)?.into_iter().filter(|j| QUEUE_STATUSES.contains(&j.status.as_str())) {
        let resume = latest_doc(c, job.id, "generated_resume")?;
        let cl = latest_doc(c, job.id, "generated_cover_letter")?;
        let need = answers(c, job.id)?.iter().filter(|a| a.status == "needs_input").count();
        out.push(QueueItem {
            resume_approved: approved(&resume),
            cover_letter_approved: approved(&cl),
            answers_needing_input: need,
            blockers: blockers(c, &job)?,
            job,
        });
    }
    let rank = |s: &str| QUEUE_STATUSES.iter().position(|x| *x == s).unwrap_or(9);
    out.sort_by(|a, b| rank(&b.job.status).cmp(&rank(&a.job.status)).then(b.job.score.cmp(&a.job.score)));
    Ok(out)
}

fn move_to(c: &Connection, job_id: i64, to: &str, note: &str) -> AppResult<Job> {
    db::change_status(c, &StatusChange { job_id, to_status: to.into(), note: note.into(), confirmation_ref: None, user_confirmed_submission: false })
}

/// Your decision on a job in the queue.
/// send → Awaiting Approval · approve → Approved · changes → Under Review · later → Documents Ready · reject → Closed
pub fn decide(c: &Connection, job_id: i64, decision: &str, note: &str) -> AppResult<Job> {
    let job = db::get_job(c, job_id)?;
    let note = note.trim();
    match decision {
        "send" => {
            refresh_proposals(c, job_id)?;
            move_to(c, job_id, "Awaiting Approval", if note.is_empty() { "Sent to review" } else { note })
        }
        "approve" => {
            if job.status != "Awaiting Approval" {
                return Err(AppError::invalid("Send the job to review first, then approve it there."));
            }
            let j = move_to(c, job_id, "Approved", if note.is_empty() { "Approved in review queue" } else { note })?;
            c.execute("UPDATE job_answers SET status = 'confirmed' WHERE job_id = ?1 AND status = 'proposed'", [job_id])?;
            Ok(j)
        }
        "changes" => {
            if note.is_empty() {
                return Err(AppError::invalid("Say what needs to change, so you remember later"));
            }
            move_to(c, job_id, "Under Review", &format!("Changes requested: {note}"))
        }
        "later" => move_to(c, job_id, "Documents Ready", if note.is_empty() { "Saved for later" } else { note }),
        "reject" => move_to(c, job_id, "Closed", &format!("You passed on this job{}", if note.is_empty() { String::new() } else { format!(": {note}") })),
        _ => Err(AppError::invalid(format!("Unknown decision “{decision}”"))),
    }
}

/// Marks an approved job In Progress and returns the official application link for the caller to open.
pub fn start_application(c: &Connection, job_id: i64) -> AppResult<String> {
    let job = db::get_job(c, job_id)?;
    if !matches!(job.status.as_str(), "Approved" | "In Progress") {
        return Err(AppError::invalid("Approve this job in the review queue before opening the application."));
    }
    let url = job
        .draft
        .apply_url
        .clone()
        .or_else(|| job.draft.posting_url.clone())
        .ok_or_else(|| AppError::invalid("This job has no application link. Add one on the job's page."))?;
    if job.status == "Approved" {
        move_to(c, job_id, "In Progress", "Opened the official application page")?;
    }
    Ok(url)
}

/// Adds a follow-up reminder after submission, unless an open one already exists.
pub fn schedule_follow_up(c: &Connection, job_id: i64) -> AppResult<()> {
    let open: i64 = c.query_row("SELECT COUNT(*) FROM reminders WHERE job_id = ?1 AND done = 0", [job_id], |r| r.get(0))?;
    if open > 0 {
        return Ok(());
    }
    let due = (db::today() + chrono::Duration::days(follow_up_days(c)?)).to_string();
    db::save_reminder(c, &crate::models::Reminder { id: None, job_id, due_date: due, note: "Follow up on your application if you haven't heard back".into(), done: false })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::JobDraft;

    fn setup() -> (Connection, i64) {
        let c = db::open_in_memory().unwrap();
        let d = JobDraft {
            title: "Intake Coordinator".into(),
            employer: "Contoso".into(),
            remote_type: "remote".into(),
            wisconsin_eligibility: "eligible".into(),
            salary_min: Some(60000.0),
            salary_max: Some(72000.0),
            salary_period: Some("year".into()),
            description: "Ability to travel up to 20% of the time.".into(),
            apply_url: Some("https://example.com/apply".into()),
            ..Default::default()
        };
        let id = db::insert_job(&c, &d, false, false).unwrap().id.unwrap();
        (c, id)
    }

    fn approve_resume(c: &Connection, job_id: i64) {
        c.execute(
            "INSERT INTO documents (job_id, kind, file_name, stored_path, sha256, version, status, approved_at, created_at)
             VALUES (?1, 'generated_resume', 'r.docx', '/tmp/r.docx', 'x', 1, 'approved', ?2, ?2)",
            params![job_id, now()],
        )
        .unwrap();
    }

    #[test]
    fn proposals_never_invent() {
        let (c, id) = setup();
        let a = refresh_proposals(&c, id).unwrap();
        let get = |k: &str| a.iter().find(|x| x.key == k).cloned();
        assert_eq!(get("work_authorized").unwrap().status, "needs_input", "no standard answer yet");
        assert!(get("work_authorized").unwrap().answer.is_empty());
        assert_eq!(get("why_interested").unwrap().status, "needs_input");
        assert!(get("salary").unwrap().answer.contains("$60,000"), "{:?}", get("salary"));
        assert!(get("travel").is_some(), "posting mentions travel");
        save_bank_answer(&c, "work_authorized", "Yes").unwrap();
        let a = refresh_proposals(&c, id).unwrap();
        let w = a.iter().find(|x| x.key == "work_authorized").unwrap();
        assert_eq!((w.answer.as_str(), w.status.as_str()), ("Yes", "proposed"));
    }

    #[test]
    fn your_edits_survive_refresh() {
        let (c, id) = setup();
        let mut s = refresh_proposals(&c, id).unwrap().into_iter().find(|x| x.key == "salary").unwrap();
        s.answer = "Negotiable".into();
        save_answer(&c, &s).unwrap();
        let s2 = refresh_proposals(&c, id).unwrap().into_iter().find(|x| x.key == "salary").unwrap();
        assert_eq!((s2.answer.as_str(), s2.status.as_str(), s2.source.as_str()), ("Negotiable", "confirmed", "You"));
    }

    #[test]
    fn nothing_proceeds_without_approval() {
        let (c, id) = setup();
        // cannot open the application or submit before approval
        assert!(start_application(&c, id).is_err());
        let sub = StatusChange { job_id: id, to_status: "Submitted".into(), note: String::new(), confirmation_ref: Some("REF-1".into()), user_confirmed_submission: true };
        assert!(db::change_status(&c, &sub).is_err(), "submission requires per-job approval");
        // cannot approve without sending to review, or with blockers
        assert!(decide(&c, id, "approve", "").is_err());
        decide(&c, id, "send", "").unwrap();
        let e = decide(&c, id, "approve", "").unwrap_err().to_string();
        assert!(e.contains("resume"), "{e}");
        approve_resume(&c, id);
        for a in answers(&c, id).unwrap().into_iter().filter(|a| a.status == "needs_input") {
            save_answer(&c, &JobAnswer { answer: "Written by me".into(), ..a }).unwrap();
        }
        assert!(blockers(&c, &db::get_job(&c, id).unwrap()).unwrap().is_empty());
        decide(&c, id, "approve", "").unwrap();
        assert!(answers(&c, id).unwrap().iter().all(|a| a.status == "confirmed"));
        assert_eq!(start_application(&c, id).unwrap(), "https://example.com/apply");
        assert_eq!(db::get_job(&c, id).unwrap().status, "In Progress");
        let j = db::change_status(&c, &sub).unwrap();
        assert_eq!(j.status, "Submitted");
        let d = db::get_job_detail(&c, id).unwrap();
        assert_eq!(d.reminders.iter().filter(|r| !r.done).count(), 1, "follow-up scheduled");
    }

    #[test]
    fn request_changes_needs_a_note() {
        let (c, id) = setup();
        decide(&c, id, "send", "").unwrap();
        assert!(decide(&c, id, "changes", " ").is_err());
        assert_eq!(decide(&c, id, "changes", "Shorter summary").unwrap().status, "Under Review");
        assert_eq!(decide(&c, id, "reject", "").unwrap().status, "Closed");
    }
}
