//! Email monitoring: copies new messages from the dedicated job-search Gmail account (read-only),
//! sorts them with rules on this Mac, turns job alerts into Discovered jobs, links employer mail
//! (sent and received) to your applications, and proposes status changes for you to accept.
//! Nothing here changes a job's status by itself, and no email content leaves this Mac.

use crate::alerts::{self, AlertJob};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::gmail::{self, RawMessage};
use crate::models::*;
use chrono::{Local, NaiveDateTime, TimeZone};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

/// Messages fetched in one sync; the rest are picked up by the next one.
pub const MAX_PER_SYNC: usize = 300;
/// When a check stopped with mail still waiting, the next one runs this soon instead of on the interval.
pub const CATCH_UP_MINUTES: i64 = 3;
const BODY_LIMIT: usize = 20_000;
const HTML_LIMIT: usize = 400_000;

static SYNCING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailMessage {
    pub id: String,
    pub thread_id: String,
    pub direction: String,
    pub from_addr: String,
    pub from_name: String,
    pub to_addrs: String,
    pub subject: String,
    pub snippet: String,
    pub sent_at: String,
    pub category: String,
    pub board: String,
    pub job_id: Option<i64>,
    pub job_label: Option<String>,
    pub job_status: Option<String>,
    pub match_note: String,
    pub match_manual: bool,
    pub suggestion: Option<String>,
    /// '' | pending | accepted | dismissed | stale (the job moved on, so the suggestion no longer applies)
    pub suggestion_state: String,
    pub alert_jobs: Vec<AlertJob>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailStatus {
    pub configured: bool,
    pub connected: bool,
    pub address: String,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
    pub interval_minutes: i64,
    pub syncing: bool,
    pub total: i64,
    pub alerts: i64,
    pub jobs_from_email: i64,
    pub linked: i64,
    pub pending: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummary {
    pub checked: i64,
    pub new_messages: i64,
    pub alerts: i64,
    pub jobs_added: i64,
    pub linked: i64,
    pub suggestions: i64,
    pub more_waiting: bool,
    /// Google's per-minute limit stopped this check early; what was read is saved.
    pub rate_limited: bool,
}

// ───────────────────────────── addresses ─────────────────────────────

static RE_ADDR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+\-']+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").unwrap());

pub fn parse_from(h: &str) -> (String, String) {
    let addr = RE_ADDR.find(h).map(|m| m.as_str().to_lowercase()).unwrap_or_default();
    let name = h.split('<').next().unwrap_or("").trim().trim_matches('"').trim().to_string();
    let name = if name.contains('@') { String::new() } else { name };
    (name, addr)
}

pub fn addresses(h: &str) -> Vec<String> {
    let mut v: Vec<String> = RE_ADDR.find_iter(h).map(|m| m.as_str().to_lowercase()).collect();
    v.dedup();
    v
}

pub fn domain(addr: &str) -> &str {
    addr.rsplit_once('@').map(|(_, d)| d).unwrap_or("")
}

/// Applicant-tracking and email services that send on an employer's behalf: the domain says
/// nothing about which employer, so the sender name and subject decide.
const RELAY_DOMAINS: [&str; 24] = [
    "greenhouse.io", "greenhouse-mail.io", "lever.co", "hire.lever.co", "myworkday.com", "workday.com", "icims.com", "smartrecruiters.com",
    "ashbyhq.com", "jobvite.com", "bamboohr.com", "breezy.hr", "workablemail.com", "workable.com", "paylocity.com", "ultipro.com",
    "adp.com", "taleo.net", "successfactors.com", "recruitee.com", "applytojob.com", "paycomonline.net", "rippling.com", "gmail.com",
];

fn is_relay(d: &str) -> bool {
    RELAY_DOMAINS.iter().any(|r| d == *r || d.ends_with(&format!(".{r}"))) || alerts::board_for(d).is_some()
}

fn no_reply(addr: &str) -> bool {
    let a = addr.to_lowercase();
    ["noreply", "no-reply", "donotreply", "do-not-reply", "mailer-daemon", "notifications@", "notification@"].iter().any(|p| a.contains(p))
}

// ───────────────────────────── classification ─────────────────────────────

static RE_REJECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bunfortunately,?\s+(?:we|after|at this time|the (?:position|role|team|hiring)|your application)|\bnot (?:to )?(?:be )?mov(?:e|ing) forward|(?:decided|chosen|elected|chose) (?:to )?(?:move forward|proceed|pursue|go forward|continue) with (?:other|another|a different)|other candidates (?:whose|who)|position has (?:now )?been filled|no longer (?:under consideration|being considered|considering)|will not be (?:moving|proceeding|advancing|offering)|regret to inform|(?:were|was|are|have) not (?:been )?selected|not a (?:match|fit) at this time",
    )
    .unwrap()
});
static RE_OFFER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:pleased|delighted|excited|happy|thrilled) to (?:extend|offer you)|\boffer letter\b|(?:formal|verbal|written|official) offer\b|offer of employment").unwrap()
});
static RE_INTERVIEW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:schedule|book|set up|arrange|coordinate)\s+(?:an?\s+|your\s+|the\s+|a time for\s+(?:an?\s+)?)?(?:interview|phone screen|video call|phone call|time to (?:talk|speak|meet))|invite you to (?:an?\s+)?(?:interview|phone screen|meet|speak)|(?:would|'d) (?:like|love) to (?:invite you|schedule|set up|speak with you|chat with you|talk with you|meet with you)|\byour availability\b|\bavailability (?:for|this|next)\b|interview (?:invitation|request|confirmation|scheduled|confirmed)|calendly\.com/|goodtime\.io|\bphone screen\b",
    )
    .unwrap()
});
static RE_ASSESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:online|skills?|pre-employment|candidate|cognitive|personality) assessment|assessment (?:link|invitation|invite)|complete (?:the|an?|this|your|our) (?:online |short |brief )?(?:assessment|skills test|questionnaire|screening questions)|hackerrank|codility|testgorilla|criteria corp|wonderlic|vervoe",
    )
    .unwrap()
});
static RE_CONFIRM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)thank(?:s| you) for (?:applying|your application|submitting|your interest in)|\b(?:application|resume) (?:has been |was |is )?(?:received|submitted|sent|complete)\b|we(?:'ve| have)? received your (?:application|resume)|your application (?:to|for|with) .{1,120}?(?:was|has been|is) (?:sent|submitted|received)|^indeed application:|\bapplication submitted\b|successfully (?:applied|submitted)",
    )
    .unwrap()
});
/// Account mail from a job board (codes, password resets): never an alert.
static RE_ACCOUNT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)verification code|with code:?\s*\d|confirm your (?:new )?email|verify your|password|sign-?in (?:attempt|alert|code)|security alert|one-time (?:code|passcode)").unwrap()
});
static RE_JOB_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:position|role|opportunit(?:y|ies)|resume|candidate|candidacy|hiring|recruit(?:er|ing)|interview|application)\b").unwrap()
});

fn head(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// What kind of message this is. Job boards are judged on the subject and opening only, because
/// their emails also carry ads and "similar jobs" that would otherwise look like news.
pub fn classify(direction: &str, from_addr: &str, board: Option<&str>, subject: &str, text: &str, alert_jobs: usize) -> &'static str {
    if direction == "out" {
        return "sent";
    }
    let scope = if board.is_some() { 600 } else { 4000 };
    let t = format!("{subject}\n{}", head(text, scope));
    if RE_REJECT.is_match(&t) {
        return "rejection";
    }
    if board.is_none() && RE_OFFER.is_match(&t) {
        return "offer";
    }
    if RE_INTERVIEW.is_match(&t) {
        return "interview";
    }
    if board.is_none() && RE_ASSESS.is_match(&t) {
        return "assessment";
    }
    if RE_CONFIRM.is_match(&t) || RE_CONFIRM.is_match(subject) {
        return "confirmation";
    }
    if board.is_some() {
        if RE_ACCOUNT.is_match(subject) {
            return "other";
        }
        return if alert_jobs > 0 { "alert" } else { "other" };
    }
    if !no_reply(from_addr) && RE_JOB_WORDS.is_match(&t) {
        return "recruiter";
    }
    "other"
}

// ───────────────────────────── matching ─────────────────────────────

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: i64,
    pub employer: String,
    pub title: String,
    pub status: String,
    pub updated_at: String,
    pub contacts: Vec<String>,
}

pub fn candidates(c: &Connection) -> AppResult<Vec<Candidate>> {
    let mut st = c.prepare("SELECT id, employer, title, status, updated_at FROM jobs WHERE is_sample = 0")?;
    let mut v = st
        .query_map([], |r| {
            Ok(Candidate { id: r.get(0)?, employer: r.get(1)?, title: r.get(2)?, status: r.get(3)?, updated_at: r.get(4)?, contacts: vec![] })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut st = c.prepare("SELECT job_id, lower(trim(email)) FROM contacts WHERE trim(email) != ''")?;
    let pairs = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    for (job, e) in pairs {
        if let Some(cand) = v.iter_mut().find(|x| x.id == job) {
            cand.contacts.push(e);
        }
    }
    Ok(v)
}

/// How far along an application is, for choosing between several jobs at one employer.
fn progress(status: &str) -> i32 {
    match status {
        "Offer" => 9,
        "Interviewing" => 8,
        "Submitted" => 7,
        "In Progress" => 6,
        "Approved" => 5,
        "Awaiting Approval" => 4,
        "Documents Ready" => 3,
        "Under Review" => 2,
        "Discovered" => 1,
        _ => 0,
    }
}

fn host_label(d: &str) -> String {
    // "careers.acmecare.example" → "careersacmecare" (everything before the last label)
    let parts: Vec<&str> = d.split('.').collect();
    let keep = if parts.len() > 1 { &parts[..parts.len() - 1] } else { &parts[..] };
    keep.join("").chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

fn word_hit(text: &str, term: &str) -> bool {
    !term.trim().is_empty() && crate::documents::term_regex(term).is_match(text)
}

/// How strongly the message points at this employer.
fn employer_score(cand: &Candidate, domains: &[&str], name: &str, subject: &str, body: &str) -> i32 {
    let tokens = crate::company::name_tokens(&cand.employer);
    let full = cand.employer.trim();
    let mut s = 0;
    if domains.iter().any(|d| {
        !is_relay(d) && {
            let h = host_label(d);
            tokens.iter().any(|t| if t.len() >= 4 { h.contains(t.as_str()) } else { d.split('.').any(|l| l == t) })
        }
    }) {
        s += 4;
    }
    let named = |t: &str| word_hit(t, full) || tokens.iter().any(|tok| tok.len() >= 5 && word_hit(t, tok));
    if !name.is_empty() && named(name) {
        s += 3;
    }
    if named(subject) {
        s += 3;
    }
    if s == 0 && named(body) {
        s += 1;
    }
    s
}

fn title_score(title: &str, subject: &str, body: &str) -> i32 {
    if word_hit(subject, title) {
        return 3;
    }
    if word_hit(body, title) {
        return 2;
    }
    let words: Vec<String> = title.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 4).map(|w| w.to_lowercase()).collect();
    let both = format!("{subject}\n{body}");
    (words.len() >= 2 && words.iter().all(|w| word_hit(&both, w))) as i32
}

pub struct MatchInput<'a> {
    pub direction: &'a str,
    pub from_addr: &'a str,
    pub from_name: &'a str,
    pub to: &'a [String],
    pub subject: &'a str,
    pub body: &'a str,
}

/// The application this message belongs to, with a note saying why.
pub fn match_job(m: &MatchInput, cands: &[Candidate], thread_job: Option<i64>) -> Option<(i64, String)> {
    if let Some(j) = thread_job.filter(|j| cands.iter().any(|c| c.id == *j)) {
        return Some((j, "Same email thread as an earlier message about this job".into()));
    }
    let people: Vec<&str> = if m.direction == "out" { m.to.iter().map(String::as_str).collect() } else { vec![m.from_addr] };
    let by_contact: Vec<&Candidate> = cands.iter().filter(|c| c.contacts.iter().any(|e| people.contains(&e.as_str()))).collect();
    if let Some(best) = by_contact.iter().max_by_key(|c| (progress(&c.status), c.updated_at.clone())) {
        let who = people.iter().find(|p| best.contacts.iter().any(|e| e == *p)).copied().unwrap_or("");
        let verb = if m.direction == "out" { "Sent to" } else { "From" };
        return Some((best.id, format!("{verb} your contact {who}")));
    }
    let domains: Vec<&str> = people.iter().map(|p| domain(p)).collect();
    let name = if m.direction == "out" { "" } else { m.from_name };
    let body = head(m.body, 4000);
    let mut scored: Vec<(i32, &Candidate, i32)> = cands
        .iter()
        .filter_map(|c| {
            let e = employer_score(c, &domains, name, m.subject, body);
            if e == 0 {
                return None;
            }
            let t = title_score(&c.title, m.subject, body);
            (e + t >= 3).then_some((e + t, c, e))
        })
        .collect();
    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(progress(&b.1.status).cmp(&progress(&a.1.status))).then(b.1.updated_at.cmp(&a.1.updated_at)));
    let (top, best, e) = scored[0];
    let ties = scored.iter().filter(|(s, c, _)| *s == top && c.employer.eq_ignore_ascii_case(&best.employer)).count();
    let mut why = vec![];
    if e >= 4 {
        why.push(if m.direction == "out" { "sent to their email domain" } else { "sent from their email domain" });
    }
    if word_hit(m.subject, &best.employer) || (!name.is_empty() && word_hit(name, &best.employer)) {
        why.push("names the employer");
    }
    if top - e >= 2 {
        why.push("names the job title");
    }
    let mut note = format!("Matched to {} — {}: {}", best.employer, best.title, if why.is_empty() { "mentions the employer".to_string() } else { why.join(", ") });
    if ties > 1 {
        note.push_str(&format!(". You have {ties} jobs at {} that fit equally; this one is furthest along. Change it if that's wrong.", best.employer));
    }
    Some((best.id, note))
}

/// The status a message suggests for the linked job, if any. Only ever a proposal.
pub fn suggestion_for(category: &str, status: &str) -> Option<&'static str> {
    match (category, status) {
        ("confirmation", "Approved" | "In Progress") => Some("Submitted"),
        ("interview", "Approved" | "In Progress" | "Submitted") => Some("Interviewing"),
        ("offer", "Submitted" | "Interviewing") => Some("Offer"),
        ("rejection", s) if !matches!(s, "Rejected" | "Withdrawn" | "Closed" | "Discovered") => Some("Rejected"),
        _ => None,
    }
}

// ───────────────────────────── storing ─────────────────────────────

fn local_time(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(db::now)
}

fn cut(s: &str, n: usize) -> String {
    head(s, n).to_string()
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Stored {
    pub new: bool,
    pub alert: bool,
    pub jobs_added: i64,
    pub linked: bool,
    pub suggestion: bool,
}

/// Tries to add one job read from an alert, and records what happened to it.
pub fn add_alert_job(c: &Connection, j: &mut AlertJob, board: &str, received: &str, check_fit: bool) {
    if j.title.trim().is_empty() || j.employer.trim().is_empty() {
        j.outcome = "incomplete".into();
        j.reason = "The company name wasn't clear in the email, so it wasn't added. Open the link to add it yourself.".into();
        return;
    }
    let link = j.url.starts_with("http").then(|| j.url.clone());
    let mut d = JobDraft {
        title: j.title.clone(),
        employer: j.employer.clone(),
        location_text: j.location.clone(),
        posting_url: link.clone(),
        apply_url: link,
        description: j.details.join("\n"),
        source: format!("{board} alert email"),
        extraction_method: "email-alert".into(),
        detection_notes: vec![format!(
            "Read from a {board} job-alert email received {received}. The title, company and location came from the email; open the posting to check them and to get the full description."
        )],
        ..Default::default()
    };
    crate::parse::enrich(&mut d);
    if check_fit {
        let p = match db::get_preferences(c) {
            Ok(p) => p,
            Err(e) => {
                j.outcome = "error".into();
                j.reason = e.to_string();
                return;
            }
        };
        if !crate::discovery::relevant(&d, &p) {
            j.outcome = "filtered".into();
            j.reason = "The title doesn't match your target titles or keywords.".into();
            return;
        }
        let ev = crate::eligibility::evaluate(&d, &p, db::today());
        if !ev.excluded_reasons.is_empty() {
            j.outcome = "excluded".into();
            j.reason = ev.excluded_reasons.join("; ");
            return;
        }
    }
    match db::insert_job(c, &d, false, false) {
        Ok(r) if r.id.is_some() => {
            j.outcome = "added".into();
            j.reason = String::new();
            j.job_id = r.id;
        }
        Ok(r) => {
            j.outcome = "duplicate".into();
            j.reason = "Already in your list.".into();
            j.job_id = r.duplicate_of.map(|x| x.id);
        }
        Err(e) => {
            j.outcome = "error".into();
            j.reason = e.to_string();
        }
    }
}

/// Sorts, links and saves one message. A message already stored is left as it is.
pub fn store(c: &Connection, raw: &RawMessage, my_addr: &str) -> AppResult<Stored> {
    let mut out = Stored::default();
    if c.query_row("SELECT 1 FROM email_messages WHERE id = ?1", [&raw.id], |_| Ok(())).optional()?.is_some() {
        return Ok(out);
    }
    let me = my_addr.to_lowercase();
    let (from_name, from_addr) = parse_from(&raw.from);
    let direction = if raw.labels.iter().any(|l| l == "SENT") || (!me.is_empty() && from_addr == me) { "out" } else { "in" };
    let to: Vec<String> = addresses(&format!("{}, {}", raw.to, raw.cc)).into_iter().filter(|a| *a != me).collect();
    let text = if raw.text.trim().is_empty() { crate::parse::html_to_text(&raw.html) } else { raw.text.replace("\r\n", "\n") };
    let board = if direction == "in" { alerts::board_for(domain(&from_addr)) } else { None };
    let mut jobs = match board {
        Some(_) if !raw.html.is_empty() => alerts::parse(&raw.html),
        _ => vec![],
    };
    let category = classify(direction, &from_addr, board, &raw.subject, &text, jobs.len());
    if category != "alert" {
        jobs.clear();
    }
    let sent_at = local_time(raw.date_ms);
    if category == "alert" {
        out.alert = true;
        for j in jobs.iter_mut() {
            add_alert_job(c, j, board.unwrap_or("Job board"), &sent_at, true);
            out.jobs_added += (j.outcome == "added") as i64;
        }
    }
    let (job_id, note) = if category == "alert" {
        (None, String::new())
    } else {
        let thread_job: Option<i64> = c
            .query_row(
                "SELECT job_id FROM email_messages WHERE thread_id = ?1 AND thread_id != '' AND job_id IS NOT NULL ORDER BY sent_at DESC LIMIT 1",
                [&raw.thread_id],
                |r| r.get(0),
            )
            .optional()?;
        let input = MatchInput { direction, from_addr: &from_addr, from_name: &from_name, to: &to, subject: &raw.subject, body: &text };
        match match_job(&input, &candidates(c)?, thread_job) {
            Some((id, n)) => (Some(id), n),
            None => (None, String::new()),
        }
    };
    let mut note = note;
    let suggestion = match job_id {
        Some(id) => {
            let status: String = c.query_row("SELECT status FROM jobs WHERE id = ?1", [id], |r| r.get(0))?;
            let s = suggestion_for(category, &status);
            if s.is_none() && category == "confirmation" && !matches!(status.as_str(), "Submitted" | "Interviewing" | "Offer") {
                note.push_str(&format!(
                    ". The employer confirmed an application, but this job is “{status}” here. Approve it in the Review queue first if you want to record it as Submitted."
                ));
            }
            s
        }
        None => None,
    };
    c.execute(
        "INSERT OR IGNORE INTO email_messages (id, thread_id, direction, from_addr, from_name, to_addrs, subject, snippet, body_text, body_html,
            sent_at, category, board, job_id, match_note, suggestion, suggestion_state, alert_jobs, fetched_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![
            raw.id,
            raw.thread_id,
            direction,
            from_addr,
            from_name,
            to.join(", "),
            raw.subject,
            raw.snippet,
            cut(&text, BODY_LIMIT),
            if board.is_some() { cut(&raw.html, HTML_LIMIT) } else { String::new() },
            sent_at,
            category,
            board.unwrap_or(""),
            job_id,
            note,
            suggestion,
            if suggestion.is_some() { "pending" } else { "" },
            serde_json::to_string(&jobs).unwrap_or_else(|_| "[]".into()),
            db::now(),
        ],
    )?;
    out.new = true;
    out.linked = job_id.is_some();
    out.suggestion = suggestion.is_some();
    Ok(out)
}

// ───────────────────────────── reading ─────────────────────────────

fn row_to_message(r: &rusqlite::Row) -> rusqlite::Result<EmailMessage> {
    let alert_json: String = r.get("alert_jobs")?;
    Ok(EmailMessage {
        id: r.get("id")?,
        thread_id: r.get("thread_id")?,
        direction: r.get("direction")?,
        from_addr: r.get("from_addr")?,
        from_name: r.get("from_name")?,
        to_addrs: r.get("to_addrs")?,
        subject: r.get("subject")?,
        snippet: r.get("snippet")?,
        sent_at: r.get("sent_at")?,
        category: r.get("category")?,
        board: r.get("board")?,
        job_id: r.get("job_id")?,
        job_label: r.get("job_label")?,
        job_status: r.get("job_status")?,
        match_note: r.get("match_note")?,
        match_manual: r.get::<_, i64>("match_manual")? != 0,
        suggestion: r.get("suggestion")?,
        suggestion_state: r.get("suggestion_state")?,
        alert_jobs: serde_json::from_str(&alert_json).unwrap_or_default(),
    })
}

const SELECT: &str = "SELECT m.*, j.employer || ' — ' || j.title AS job_label, j.status AS job_status
    FROM email_messages m LEFT JOIN jobs j ON j.id = m.job_id";

/// A pending suggestion the job has since moved past is shown as stale, not offered.
fn freshen(mut m: EmailMessage) -> EmailMessage {
    if m.suggestion_state == "pending" {
        let still = match (&m.suggestion, &m.job_status) {
            (Some(s), Some(st)) => suggestion_for(&m.category, st) == Some(s.as_str()),
            _ => false,
        };
        if !still {
            m.suggestion_state = "stale".into();
        }
    }
    m
}

/// `filter`: all | pending | alert | linked | unlinked | job:<id>
pub fn list(c: &Connection, filter: &str, limit: i64) -> AppResult<Vec<EmailMessage>> {
    let (cond, arg): (&str, Option<i64>) = match filter {
        "pending" => ("m.suggestion_state = 'pending'", None),
        "alert" => ("m.category = 'alert'", None),
        "linked" => ("m.job_id IS NOT NULL", None),
        "unlinked" => ("m.job_id IS NULL AND m.category NOT IN ('alert', 'other')", None),
        f if f.starts_with("job:") => ("m.job_id = ?2", f[4..].parse().ok()),
        _ => ("1", None),
    };
    let sql = format!("{SELECT} WHERE {cond} ORDER BY m.sent_at DESC LIMIT ?1");
    let mut st = c.prepare(&sql)?;
    let rows = match arg {
        Some(id) => st.query_map(params![limit, id], row_to_message)?.collect::<Result<Vec<_>, _>>()?,
        None => st.query_map(params![limit], row_to_message)?.collect::<Result<Vec<_>, _>>()?,
    };
    let v: Vec<EmailMessage> = rows.into_iter().map(freshen).collect();
    Ok(if filter == "pending" { v.into_iter().filter(|m| m.suggestion_state == "pending").collect() } else { v })
}

pub fn get(c: &Connection, id: &str) -> AppResult<EmailMessage> {
    let sql = format!("{SELECT} WHERE m.id = ?1");
    c.query_row(&sql, [id], row_to_message).optional()?.map(freshen).ok_or_else(|| AppError::invalid("That email is no longer stored"))
}

pub fn body(c: &Connection, id: &str) -> AppResult<String> {
    Ok(c.query_row("SELECT body_text FROM email_messages WHERE id = ?1", [id], |r| r.get(0)).optional()?.unwrap_or_default())
}

fn setting(c: &Connection, k: &str) -> String {
    db::get_setting(c, k).ok().flatten().unwrap_or_default()
}

pub fn interval_minutes(c: &Connection) -> i64 {
    setting(c, "gmail_interval_minutes").parse().unwrap_or(30)
}

pub fn set_interval(c: &Connection, minutes: i64) -> AppResult<()> {
    if !(0..=1440).contains(&minutes) {
        return Err(AppError::invalid("Choose between 0 (off) and 1440 minutes"));
    }
    db::set_setting(c, "gmail_interval_minutes", &minutes.to_string())
}

pub fn status(c: &Connection) -> AppResult<EmailStatus> {
    let address = setting(c, "gmail_address");
    let count = |sql: &str| -> AppResult<i64> { Ok(c.query_row(sql, [], |r| r.get(0))?) };
    let opt = |k: &str| Some(setting(c, k)).filter(|s| !s.is_empty());
    Ok(EmailStatus {
        configured: crate::secrets::has(gmail::CLIENT_ID) && crate::secrets::has(gmail::CLIENT_SECRET),
        connected: !address.is_empty() && crate::secrets::has(gmail::REFRESH),
        address,
        last_sync: opt("gmail_last_sync"),
        last_error: opt("gmail_last_error"),
        interval_minutes: interval_minutes(c),
        syncing: SYNCING.load(Ordering::SeqCst),
        total: count("SELECT COUNT(*) FROM email_messages")?,
        alerts: count("SELECT COUNT(*) FROM email_messages WHERE category = 'alert'")?,
        jobs_from_email: count("SELECT COUNT(*) FROM jobs WHERE extraction_method = 'email-alert'")?,
        linked: count("SELECT COUNT(*) FROM email_messages WHERE job_id IS NOT NULL")?,
        pending: list(c, "pending", 500)?.len() as i64,
    })
}

// ───────────────────────────── your decisions ─────────────────────────────

/// You accepted the proposed status change: it goes through the normal status rules.
pub fn accept(c: &Connection, id: &str) -> AppResult<Job> {
    let m = get(c, id)?;
    let (Some(job_id), Some(to)) = (m.job_id, m.suggestion.clone()) else {
        return Err(AppError::invalid("This email has no status change to accept"));
    };
    if m.suggestion_state != "pending" {
        return Err(AppError::invalid(match m.suggestion_state.as_str() {
            "stale" => "The job's status has changed since this email arrived, so this suggestion no longer applies",
            _ => "This suggestion was already handled",
        }));
    }
    let who = if m.from_name.is_empty() { m.from_addr.clone() } else { m.from_name.clone() };
    let job = db::change_status(
        c,
        &StatusChange {
            job_id,
            to_status: to.clone(),
            note: format!("From email “{}” ({who}, {}), accepted by you", m.subject, m.sent_at),
            confirmation_ref: None,
            user_confirmed_submission: to == "Submitted",
        },
    )?;
    c.execute("UPDATE email_messages SET suggestion_state = 'accepted' WHERE id = ?1", [id])?;
    db::log(c, "info", "email", &format!("Accepted email suggestion: job #{job_id} → {to}"));
    Ok(job)
}

pub fn dismiss(c: &Connection, id: &str) -> AppResult<()> {
    c.execute("UPDATE email_messages SET suggestion_state = 'dismissed' WHERE id = ?1 AND suggestion_state = 'pending'", [id])?;
    Ok(())
}

/// You linked the message to a job (or unlinked it). A new suggestion is worked out for the new job.
pub fn set_link(c: &Connection, id: &str, job_id: Option<i64>) -> AppResult<EmailMessage> {
    let m = get(c, id)?;
    let (suggestion, note) = match job_id {
        Some(j) => {
            let job = db::get_job(c, j)?;
            (suggestion_for(&m.category, &job.status), format!("Linked by you to {} — {}", job.draft.employer, job.draft.title))
        }
        None => (None, "Unlinked by you".to_string()),
    };
    c.execute(
        "UPDATE email_messages SET job_id = ?1, match_manual = 1, match_note = ?2, suggestion = ?3, suggestion_state = ?4 WHERE id = ?5",
        params![job_id, note, suggestion, if suggestion.is_some() { "pending" } else { "" }, id],
    )?;
    get(c, id)
}

/// Adds a job from an alert that was left out (filtered, excluded or incomplete), on your say-so.
pub fn add_alert_anyway(c: &Connection, id: &str, index: usize, edited: Option<AlertJob>) -> AppResult<EmailMessage> {
    let m = get(c, id)?;
    let mut jobs = m.alert_jobs.clone();
    let j = jobs.get_mut(index).ok_or_else(|| AppError::invalid("That job is not in this email"))?;
    if j.outcome == "added" || j.outcome == "duplicate" {
        return Err(AppError::invalid("That job is already in your list"));
    }
    if let Some(e) = edited {
        j.title = e.title.trim().to_string();
        j.employer = e.employer.trim().to_string();
        j.location = e.location.trim().to_string();
    }
    add_alert_job(c, j, if m.board.is_empty() { "Job board" } else { &m.board }, &m.sent_at, false);
    if j.outcome == "incomplete" || j.outcome == "error" {
        return Err(AppError::invalid(j.reason.clone()));
    }
    c.execute("UPDATE email_messages SET alert_jobs = ?1 WHERE id = ?2", params![serde_json::to_string(&jobs).unwrap_or_default(), id])?;
    get(c, id)
}

/// Reads a stored job-board email again with the current rules (after a parser fix). Jobs already
/// added stay as they are; new ones go through the usual checks.
pub fn reread(c: &Connection, id: &str) -> AppResult<EmailMessage> {
    let m = get(c, id)?;
    let html: String = c.query_row("SELECT body_html FROM email_messages WHERE id = ?1", [id], |r| r.get(0))?;
    if html.is_empty() {
        return Err(AppError::invalid("Only job-board emails can be read again"));
    }
    let text = body(c, id)?;
    let mut jobs = alerts::parse(&html);
    let category = classify(&m.direction, &m.from_addr, Some(m.board.as_str()).filter(|b| !b.is_empty()), &m.subject, &text, jobs.len());
    if category != "alert" {
        jobs.clear();
    }
    for j in jobs.iter_mut() {
        if let Some(old) = m.alert_jobs.iter().find(|o| o.title == j.title && o.employer == j.employer && o.job_id.is_some()) {
            *j = old.clone();
        } else {
            add_alert_job(c, j, &m.board, &m.sent_at, true);
        }
    }
    c.execute(
        "UPDATE email_messages SET category = ?1, alert_jobs = ?2 WHERE id = ?3",
        params![category, serde_json::to_string(&jobs).unwrap_or_default(), id],
    )?;
    get(c, id)
}

/// Reads every stored job-board email again with the current rules. Returns (emails, jobs added).
pub fn reread_all(c: &Connection) -> AppResult<(usize, i64)> {
    let ids: Vec<String> = {
        let mut st = c.prepare("SELECT id FROM email_messages WHERE body_html != '' ORDER BY sent_at")?;
        let v = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;
        v
    };
    let mut added = 0;
    for id in &ids {
        let before = get(c, id)?.alert_jobs.iter().filter(|j| j.outcome == "added").count() as i64;
        let after = reread(c, id)?.alert_jobs.iter().filter(|j| j.outcome == "added").count() as i64;
        added += (after - before).max(0);
    }
    db::log(c, "info", "email", &format!("Read {} job-board emails again: {added} more jobs added", ids.len()));
    Ok((ids.len(), added))
}

/// Removes every stored email from this Mac (Gmail itself is untouched). Jobs already added stay.
pub fn forget_all(c: &Connection) -> AppResult<usize> {
    let n = c.execute("DELETE FROM email_messages", [])?;
    db::set_setting(c, "gmail_last_sync", "")?;
    Ok(n)
}

// ───────────────────────────── syncing ─────────────────────────────

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        SYNCING.store(false, Ordering::SeqCst);
    }
}

fn record_error(db_path: &Path, e: &AppError) {
    if let Ok(c) = db::open(db_path) {
        let _ = db::set_setting(&c, "gmail_last_error", &e.to_string());
        db::log(&c, "error", "email", &format!("Email check failed: {e}"));
    }
}

/// The Gmail search for this sync: everything since a day before the last good sync (overlap is
/// harmless, messages already stored are skipped), or the last N days the first time.
pub fn query(last_sync: &str, first_days: i64) -> String {
    match NaiveDateTime::parse_from_str(last_sync, "%Y-%m-%d %H:%M:%S").ok().and_then(|t| Local.from_local_datetime(&t).single()) {
        Some(t) => format!("after:{}", t.timestamp() - 86_400),
        None => format!("newer_than:{}d", first_days.clamp(1, 365)),
    }
}

pub async fn sync(db_path: &Path, trigger: &str) -> AppResult<SyncSummary> {
    if SYNCING.swap(true, Ordering::SeqCst) {
        return Err(AppError::invalid("An email check is already running"));
    }
    let _guard = Guard;
    let (address, last, days) = {
        let c = db::open(db_path)?;
        let _ = db::set_setting(&c, "gmail_last_attempt", &db::now());
        (setting(&c, "gmail_address"), setting(&c, "gmail_last_sync"), setting(&c, "gmail_first_sync_days").parse().unwrap_or(30))
    };
    if address.is_empty() {
        return Err(AppError::invalid("Gmail is not connected"));
    }
    let run = async {
        let token = gmail::access_token().await?;
        let ids = gmail::list_ids(&token, &query(&last, days), 2000).await?;
        let fresh: Vec<String> = {
            let c = db::open(db_path)?;
            let mut v = vec![];
            for id in ids.iter() {
                if c.query_row("SELECT 1 FROM email_messages WHERE id = ?1", [id], |_| Ok(())).optional()?.is_none() {
                    v.push(id.clone());
                }
            }
            v
        };
        let c = db::open(db_path)?;
        let mut s = SyncSummary { checked: ids.len() as i64, more_waiting: fresh.len() > MAX_PER_SYNC, ..Default::default() };
        // Gmail lists newest first. Read the oldest first so replies link to their thread, and save
        // each email as soon as it arrives, so an interruption never loses what was already read.
        for id in fresh.iter().rev().take(MAX_PER_SYNC) {
            let v = match gmail::get_message(&token, id).await {
                Ok(v) => v,
                Err(e) if gmail::is_rate_limited(&e) => {
                    s.more_waiting = true;
                    s.rate_limited = true;
                    break;
                }
                Err(e) => return Err(e),
            };
            let Some(m) = gmail::parse_message(&v) else { continue };
            if m.labels.iter().any(|l| matches!(l.as_str(), "DRAFT" | "SPAM" | "TRASH" | "CHAT")) {
                continue;
            }
            match store(&c, &m, &address) {
                Ok(r) => {
                    s.new_messages += r.new as i64;
                    s.alerts += r.alert as i64;
                    s.jobs_added += r.jobs_added;
                    s.linked += r.linked as i64;
                    s.suggestions += r.suggestion as i64;
                }
                Err(e) => db::log(&c, "warn", "email", &format!("Skipped one email: {e}")),
            }
        }
        let more_waiting = s.more_waiting;
        if !more_waiting {
            db::set_setting(&c, "gmail_last_sync", &db::now())?;
        }
        db::set_setting(&c, "gmail_more_waiting", if more_waiting { "1" } else { "" })?;
        db::set_setting(&c, "gmail_last_error", "")?;
        db::log(
            &c,
            "info",
            "email",
            &format!(
                "Email check ({trigger}): {} new of {} checked, {} alerts, {} jobs added, {} linked to applications, {} suggestions{}",
                s.new_messages, s.checked, s.alerts, s.jobs_added, s.linked, s.suggestions,
                if s.rate_limited { "; Gmail asked to slow down, continuing in a few minutes" } else if more_waiting { "; more waiting, continuing in a few minutes" } else { "" }
            ),
        );
        Ok::<_, AppError>(s)
    };
    let r = run.await;
    if let Err(e) = &r {
        record_error(db_path, e);
    }
    r
}

/// Is a scheduled check due? Uses the last attempt too, so a failing check isn't retried every minute.
pub fn due(c: &Connection, now: NaiveDateTime) -> bool {
    let mins = interval_minutes(c);
    if mins <= 0 || setting(c, "gmail_address").is_empty() || SYNCING.load(Ordering::SeqCst) {
        return false;
    }
    let last = |k: &str| NaiveDateTime::parse_from_str(&setting(c, k), "%Y-%m-%d %H:%M:%S").ok();
    let newest = last("gmail_last_sync").into_iter().chain(last("gmail_last_attempt")).max();
    let wait = if setting(c, "gmail_more_waiting") == "1" { CATCH_UP_MINUTES.min(mins) } else { mins };
    newest.map(|t| (now - t).num_minutes() >= wait).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: i64, employer: &str, title: &str, status: &str) -> Candidate {
        Candidate { id, employer: employer.into(), title: title.into(), status: status.into(), updated_at: "2026-11-01 10:00:00".into(), contacts: vec![] }
    }

    fn input<'a>(dir: &'a str, from: &'a str, name: &'a str, to: &'a [String], subject: &'a str, body: &'a str) -> MatchInput<'a> {
        MatchInput { direction: dir, from_addr: from, from_name: name, to, subject, body }
    }

    #[test]
    fn parses_addresses() {
        assert_eq!(parse_from("\"Acme Care Talent\" <Talent@AcmeCare.example>"), ("Acme Care Talent".into(), "talent@acmecare.example".into()));
        assert_eq!(parse_from("jobs@northwind.example"), (String::new(), "jobs@northwind.example".into()));
        assert_eq!(addresses("A <a@x.example>, b@y.example"), vec!["a@x.example", "b@y.example"]);
        assert_eq!(domain("a@careers.acme.example"), "careers.acme.example");
    }

    #[test]
    fn classifies_common_employer_emails() {
        let c = |s: &str, b: &str| classify("in", "talent@acmecare.example", None, s, b, 0);
        assert_eq!(c("Thank you for applying to Acme Care", "We received your application and will review it. If selected for an interview we will contact you."), "confirmation");
        assert_eq!(c("Your application", "Thank you for your interest. Unfortunately, we have decided to move forward with other candidates."), "rejection");
        assert_eq!(c("Next steps", "We'd like to schedule an interview. Please share your availability for next week."), "interview");
        assert_eq!(c("Acme Care assessment", "Please complete the online assessment within 3 days."), "assessment");
        assert_eq!(c("Good news", "We are pleased to extend an offer of employment."), "offer");
        assert_eq!(c("Quick question about the role", "I'm a recruiter at Acme Care and saw your resume."), "recruiter");
        assert_eq!(c("Our newsletter", "Read our latest blog post."), "other");
        assert_eq!(classify("out", "me@example.com", None, "Following up", "", 0), "sent");
    }

    #[test]
    fn job_board_emails_are_judged_on_their_opening() {
        let long_ad = format!("{}\nUnfortunately, we have decided to move forward with other candidates.", "x ".repeat(700));
        assert_eq!(classify("in", "jobalerts-noreply@linkedin.com", Some("LinkedIn"), "Intake manager: 12 new jobs", &long_ad, 12), "alert");
        assert_eq!(classify("in", "jobs-noreply@linkedin.com", Some("LinkedIn"), "Your application was sent to Acme Care", "", 6), "confirmation");
        assert_eq!(classify("in", "indeedapply@indeed.com", Some("Indeed"), "Indeed Application: Call Center Supervisor", "", 0), "confirmation");
        assert_eq!(classify("in", "news@indeed.com", Some("Indeed"), "Career tips", "Read more", 0), "other");
        assert_eq!(classify("in", "no-reply@indeed.com", Some("Indeed"), "Confirm your new email address on Indeed with code: 123456.", "", 2), "other");
    }

    #[test]
    fn matches_by_domain_name_and_title() {
        let cands = vec![
            cand(1, "Acme Care", "Intake Manager", "Submitted"),
            cand(2, "Northwind Health", "Recruiting Coordinator", "Submitted"),
        ];
        let none: Vec<String> = vec![];
        let m = match_job(&input("in", "talent@acmecare.example", "Acme Care Talent", &none, "Your application", "Thanks"), &cands, None).unwrap();
        assert_eq!(m.0, 1);
        assert!(m.1.contains("email domain"), "{}", m.1);
        // An applicant-tracking relay: the domain says nothing, the sender name does.
        let m = match_job(&input("in", "no-reply@us.greenhouse-mail.io", "Northwind Health Hiring Team", &none, "Thank you for applying", ""), &cands, None).unwrap();
        assert_eq!(m.0, 2);
        // Outbound: to their domain.
        let to = vec!["pat@northwind.example".to_string()];
        let m = match_job(&input("out", "me@example.com", "", &to, "Following up on the Recruiting Coordinator role", ""), &cands, None).unwrap();
        assert_eq!(m.0, 2);
        // Nothing to go on.
        assert!(match_job(&input("in", "hello@contoso.example", "Contoso", &none, "Hello", "Hi"), &cands, None).is_none());
        // A body that merely mentions an employer isn't enough.
        assert!(match_job(&input("in", "news@contoso.example", "Contoso News", &none, "Industry news", "Acme Care opened a clinic"), &cands, None).is_none());
    }

    #[test]
    fn several_jobs_at_one_employer_prefer_title_then_progress() {
        let cands = vec![
            cand(1, "Acme Care", "Intake Manager", "Discovered"),
            cand(2, "Acme Care", "Call Center Supervisor", "Submitted"),
            cand(3, "Acme Care", "Patient Access Lead", "Interviewing"),
        ];
        let none: Vec<String> = vec![];
        let m = match_job(&input("in", "talent@acmecare.example", "Acme Care", &none, "Your Intake Manager application", ""), &cands, None).unwrap();
        assert_eq!(m.0, 1);
        let m = match_job(&input("in", "talent@acmecare.example", "Acme Care", &none, "An update", ""), &cands, None).unwrap();
        assert_eq!(m.0, 3);
        assert!(m.1.contains("3 jobs at Acme Care"), "{}", m.1);
    }

    #[test]
    fn contacts_and_threads_win() {
        let mut a = cand(1, "Acme Care", "Intake Manager", "Submitted");
        a.contacts.push("pat@recruit.example".into());
        let cands = vec![a, cand(2, "Northwind Health", "Coordinator", "Submitted")];
        let none: Vec<String> = vec![];
        let m = match_job(&input("in", "pat@recruit.example", "Pat", &none, "Hi", ""), &cands, None).unwrap();
        assert_eq!(m.0, 1);
        assert!(m.1.contains("your contact"));
        let m = match_job(&input("in", "x@y.example", "", &none, "Re: hi", ""), &cands, Some(2)).unwrap();
        assert_eq!(m.0, 2);
    }

    #[test]
    fn suggestions_follow_the_status_rules() {
        assert_eq!(suggestion_for("confirmation", "Approved"), Some("Submitted"));
        assert_eq!(suggestion_for("confirmation", "Discovered"), None);
        assert_eq!(suggestion_for("rejection", "Interviewing"), Some("Rejected"));
        assert_eq!(suggestion_for("rejection", "Rejected"), None);
        assert_eq!(suggestion_for("rejection", "Discovered"), None);
        assert_eq!(suggestion_for("interview", "Submitted"), Some("Interviewing"));
        assert_eq!(suggestion_for("offer", "Interviewing"), Some("Offer"));
        assert_eq!(suggestion_for("other", "Submitted"), None);
    }

    #[test]
    fn first_sync_looks_back_then_continues_from_the_last() {
        assert_eq!(query("", 30), "newer_than:30d");
        assert!(query("2026-11-05 09:00:00", 30).starts_with("after:"));
    }

    fn raw(id: &str, thread: &str, from: &str, subject: &str, text: &str, html: &str) -> RawMessage {
        RawMessage {
            id: id.into(),
            thread_id: thread.into(),
            labels: vec!["INBOX".into()],
            from: from.into(),
            to: "Me <me@example.com>".into(),
            subject: subject.into(),
            date_ms: 1_762_300_000_000,
            text: text.into(),
            html: html.into(),
            ..Default::default()
        }
    }

    fn approved_job(c: &Connection, employer: &str, title: &str) -> i64 {
        let d = JobDraft { title: title.into(), employer: employer.into(), description: "Remote role".into(), ..Default::default() };
        let id = db::insert_job(c, &d, false, false).unwrap().id.unwrap();
        c.execute("UPDATE jobs SET status = 'Approved' WHERE id = ?1", [id]).unwrap();
        id
    }

    #[test]
    fn stores_links_and_suggests_without_changing_status() {
        let c = db::open_in_memory().unwrap();
        let job = approved_job(&c, "Acme Care", "Intake Manager");
        let r = store(&c, &raw("m1", "t1", "Acme Care Talent <talent@acmecare.example>", "Thank you for applying to Acme Care", "We received your application.", ""), "me@example.com").unwrap();
        assert!(r.new && r.linked && r.suggestion);
        assert_eq!(db::get_job(&c, job).unwrap().status, "Approved", "nothing changes until you accept");
        // Stored twice → ignored.
        assert!(!store(&c, &raw("m1", "t1", "x@y.example", "x", "", ""), "me@example.com").unwrap().new);
        // A reply in the same thread links by thread.
        let r = store(&c, &raw("m2", "t1", "Pat <pat@other.example>", "Re: your application", "Thanks", ""), "me@example.com").unwrap();
        assert!(r.linked);
        let pending = list(&c, "pending", 50).unwrap();
        assert_eq!(pending.len(), 1);
        let j = accept(&c, "m1").unwrap();
        assert_eq!(j.status, "Submitted");
        assert_eq!(j.confirmation_method.as_deref(), Some("user_confirmed"));
        assert!(accept(&c, "m1").is_err(), "a suggestion is accepted once");
        assert_eq!(list(&c, &format!("job:{job}"), 50).unwrap().len(), 2);
    }

    #[test]
    fn a_suggestion_goes_stale_when_the_job_moves_on() {
        let c = db::open_in_memory().unwrap();
        let job = approved_job(&c, "Acme Care", "Intake Manager");
        store(&c, &raw("m1", "t1", "talent@acmecare.example", "Thank you for applying", "Received.", ""), "me@example.com").unwrap();
        c.execute("UPDATE jobs SET status = 'Withdrawn' WHERE id = ?1", [job]).unwrap();
        assert!(list(&c, "pending", 50).unwrap().is_empty());
        assert_eq!(get(&c, "m1").unwrap().suggestion_state, "stale");
        assert!(accept(&c, "m1").is_err());
    }

    #[test]
    fn manual_links_replace_the_match() {
        let c = db::open_in_memory().unwrap();
        let a = approved_job(&c, "Acme Care", "Intake Manager");
        store(&c, &raw("m1", "t1", "pat@recruiting-agency.example", "Interview request", "We'd like to schedule an interview for the role.", ""), "me@example.com").unwrap();
        assert!(get(&c, "m1").unwrap().job_id.is_none());
        let m = set_link(&c, "m1", Some(a)).unwrap();
        assert_eq!(m.job_id, Some(a));
        assert!(m.match_manual);
        assert_eq!(m.suggestion.as_deref(), Some("Interviewing"));
        let m = set_link(&c, "m1", None).unwrap();
        assert!(m.job_id.is_none() && m.suggestion.is_none());
    }

    #[test]
    fn alerts_add_jobs_and_record_what_happened() {
        let c = db::open_in_memory().unwrap();
        let mut p = db::get_preferences(&c).unwrap();
        p.target_titles = vec!["Intake Manager".into()];
        db::save_preferences(&c, &p).unwrap();
        let html = r#"<table>
          <tr><td><a href="https://www.linkedin.com/comm/jobs/view/4012345678/?t=1">Intake Manager</a><p>Acme Care · Remote</p></td></tr>
          <tr><td><a href="https://www.linkedin.com/comm/jobs/view/4012345679/?t=1">Forklift Operator</a><p>Contoso Logistics · Remote</p></td></tr>
        </table>"#;
        let r = store(&c, &raw("a1", "ta", "LinkedIn Job Alerts <jobalerts-noreply@linkedin.com>", "Intake Manager: new jobs", "", html), "me@example.com").unwrap();
        assert!(r.alert);
        assert_eq!(r.jobs_added, 1);
        let m = get(&c, "a1").unwrap();
        assert_eq!(m.category, "alert");
        assert_eq!(m.alert_jobs[0].outcome, "added");
        assert_eq!(m.alert_jobs[1].outcome, "filtered");
        let job = db::get_job(&c, m.alert_jobs[0].job_id.unwrap()).unwrap();
        assert_eq!(job.status, "Discovered");
        assert_eq!(job.draft.source, "LinkedIn alert email");
        // Same alert again (another email) → duplicate, not a second job.
        store(&c, &raw("a2", "tb", "jobalerts-noreply@linkedin.com", "Intake Manager: new jobs", "", html), "me@example.com").unwrap();
        assert_eq!(get(&c, "a2").unwrap().alert_jobs[0].outcome, "duplicate");
        // You can add the filtered one anyway.
        let m = add_alert_anyway(&c, "a1", 1, None).unwrap();
        assert_eq!(m.alert_jobs[1].outcome, "added");
        // Re-reading keeps what was added.
        let m = reread(&c, "a1").unwrap();
        assert_eq!(m.alert_jobs.iter().filter(|j| j.outcome == "added").count(), 2);
    }

    #[test]
    fn sent_mail_is_tracked_as_outbound() {
        let c = db::open_in_memory().unwrap();
        let job = approved_job(&c, "Northwind Health", "Recruiting Coordinator");
        let mut m = raw("s1", "ts", "Me <me@example.com>", "Following up on my Recruiting Coordinator application", "Hello", "");
        m.labels = vec!["SENT".into()];
        m.to = "Pat <pat@northwind.example>".into();
        let r = store(&c, &m, "me@example.com").unwrap();
        assert!(r.linked && !r.suggestion);
        let s = get(&c, "s1").unwrap();
        assert_eq!(s.direction, "out");
        assert_eq!(s.category, "sent");
        assert_eq!(s.job_id, Some(job));
        assert_eq!(s.to_addrs, "pat@northwind.example");
    }

    #[test]
    fn schedule_respects_interval_and_failures() {
        let c = db::open_in_memory().unwrap();
        let now = NaiveDateTime::parse_from_str("2026-11-05 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        assert!(!due(&c, now), "not connected");
        db::set_setting(&c, "gmail_address", "me@example.com").unwrap();
        assert!(due(&c, now));
        db::set_setting(&c, "gmail_last_attempt", "2026-11-05 11:50:00").unwrap();
        assert!(!due(&c, now), "a recent failed attempt waits for the interval");
        db::set_setting(&c, "gmail_more_waiting", "1").unwrap();
        assert!(due(&c, now), "a backlog is picked up again after a few minutes");
        db::set_setting(&c, "gmail_more_waiting", "").unwrap();
        set_interval(&c, 0).unwrap();
        db::set_setting(&c, "gmail_last_attempt", "").unwrap();
        assert!(!due(&c, now), "turned off");
    }
}
