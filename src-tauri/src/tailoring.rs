//! Position-specific resumes and cover letters built only from verified facts.
//!
//! Integrity by construction:
//! * Employers, titles, locations, dates, education and certifications are rendered by the app
//!   straight from the profile — the model never writes them.
//! * Every sentence or bullet the model writes must cite fact IDs ("E0.3", "K12"). The claim
//!   checker blocks lines with no valid citation, numbers that are not in the cited facts, and
//!   tools/skills/certifications that are not in the verified profile.

use crate::documents::SKILL_VOCABULARY;
use crate::models::*;
use crate::scoring::{has_evidence, CERT_TERMS};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::LazyLock;

// ───────────────────────────── facts ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct PositionRef {
    pub id: String,
    pub exp: Experience,
}

pub fn positions(p: &ProfileBundle) -> Vec<PositionRef> {
    p.experience.iter().enumerate().map(|(i, e)| PositionRef { id: format!("P{i}"), exp: e.clone() }).collect()
}

/// Citable facts from the verified profile. Contact details are deliberately excluded.
pub fn facts(p: &ProfileBundle) -> Vec<Fact> {
    let mut f = vec![];
    if !p.basics.summary.trim().is_empty() {
        f.push(Fact { id: "S1".into(), text: format!("Professional summary: {}", p.basics.summary.trim()) });
    }
    if !p.basics.headline.trim().is_empty() {
        f.push(Fact { id: "H1".into(), text: format!("Headline: {}", p.basics.headline.trim()) });
    }
    for (i, e) in p.experience.iter().enumerate() {
        f.push(Fact { id: format!("P{i}"), text: format!("Position: {} at {} ({})", e.title, e.employer, dated(e)) });
        for (j, line) in e.achievements.lines().map(|l| l.trim().trim_start_matches(['•', '-', '*']).trim()).filter(|l| !l.is_empty()).enumerate() {
            f.push(Fact { id: format!("E{i}.{j}"), text: format!("[{}, {}] {}", e.title, e.employer, line) });
        }
    }
    for (i, s) in p.skills.iter().enumerate() {
        f.push(Fact { id: format!("K{i}"), text: format!("Verified skill: {}{}", s.name, if s.category.is_empty() { String::new() } else { format!(" ({})", s.category) }) });
    }
    for (i, c) in p.certifications.iter().enumerate() {
        f.push(Fact { id: format!("C{i}"), text: format!("Certification: {}{}{}", c.name, if c.issuer.is_empty() { String::new() } else { format!(", {}", c.issuer) }, if c.date_earned.is_empty() { String::new() } else { format!(", {}", c.date_earned) }) });
    }
    for (i, e) in p.education.iter().enumerate() {
        f.push(Fact { id: format!("D{i}"), text: format!("Education: {} {} {}{}", e.degree, e.field, e.institution, if e.minor.is_empty() { String::new() } else { format!(", minor {}", e.minor) }) });
    }
    for (i, a) in p.accomplishments.iter().enumerate() {
        f.push(Fact { id: format!("A{i}"), text: format!("Accomplishment: {}: {}", a.title, a.description) });
    }
    f
}

fn month_name(ym: &str) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let t = ym.trim();
    match (t.get(..4), t.get(5..7).and_then(|m| m.parse::<usize>().ok())) {
        (Some(y), Some(m)) if (1..=12).contains(&m) => format!("{} {y}", M[m - 1]),
        (Some(y), _) if y.chars().all(|c| c.is_ascii_digit()) => y.to_string(),
        _ => t.to_string(),
    }
}

/// Roles that ended this many years ago or more are listed without dates, so a reader (or a
/// screening tool) can't count back to the start of a long career.
pub const DATED_ROLE_YEARS: i32 = 15;

/// The dates to print for a role, or None for an older role whose dates are left off.
pub fn shown_dates(e: &Experience) -> Option<String> {
    shown_dates_in(e, chrono::Datelike::year(&chrono::Local::now().date_naive()))
}

pub fn shown_dates_in(e: &Experience, this_year: i32) -> Option<String> {
    let end_year: Option<i32> = e.end_date.get(..4).and_then(|y| y.parse().ok());
    match end_year {
        Some(y) if !e.is_current && y <= this_year - DATED_ROLE_YEARS => None,
        _ => Some(date_range(e)),
    }
}

fn dated(e: &Experience) -> String {
    shown_dates(e).unwrap_or_else(|| "earlier career; dates not shown".into())
}

pub fn date_range(e: &Experience) -> String {
    let end = if e.is_current { "Present".to_string() } else { month_name(&e.end_date) };
    format!("{} – {}", month_name(&e.start_date), end).trim_matches([' ', '–']).to_string()
}

// ───────────────────────────── model output shapes ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Line {
    pub text: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PositionContent {
    pub position_id: String,
    pub include: bool,
    pub bullets: Vec<Line>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ResumeContent {
    pub headline: String,
    pub summary: Line,
    pub skills: Vec<String>,
    pub positions: Vec<PositionContent>,
    /// 0–3 accomplishments shown under "Selected Projects"; the title comes from the profile.
    pub projects: Vec<ProjectContent>,
    pub notes_for_candidate: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ProjectContent {
    pub accomplishment_id: String,
    pub text: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct CoverLetterContent {
    pub greeting: String,
    pub paragraphs: Vec<Line>,
    pub closing: String,
    pub notes_for_candidate: String,
}

fn line_schema() -> Value {
    json!({"type": "object", "properties": {
        "text": {"type": "string"},
        "sources": {"type": "array", "items": {"type": "string"}}
    }, "required": ["text", "sources"], "additionalProperties": false})
}

pub fn resume_schema() -> Value {
    json!({"type": "object", "properties": {
        "headline": {"type": "string"},
        "summary": line_schema(),
        "skills": {"type": "array", "items": {"type": "string"}},
        "positions": {"type": "array", "items": {"type": "object", "properties": {
            "position_id": {"type": "string"},
            "include": {"type": "boolean"},
            "bullets": {"type": "array", "items": line_schema()}
        }, "required": ["position_id", "include", "bullets"], "additionalProperties": false}},
        "projects": {"type": "array", "items": {"type": "object", "properties": {
            "accomplishment_id": {"type": "string"},
            "text": {"type": "string"},
            "sources": {"type": "array", "items": {"type": "string"}}
        }, "required": ["accomplishment_id", "text", "sources"], "additionalProperties": false}},
        "notes_for_candidate": {"type": "string"}
    }, "required": ["headline", "summary", "skills", "positions", "projects", "notes_for_candidate"], "additionalProperties": false})
}

pub fn cover_letter_schema() -> Value {
    json!({"type": "object", "properties": {
        "greeting": {"type": "string"},
        "paragraphs": {"type": "array", "items": line_schema()},
        "closing": {"type": "string"},
        "notes_for_candidate": {"type": "string"}
    }, "required": ["greeting", "paragraphs", "closing", "notes_for_candidate"], "additionalProperties": false})
}

// ───────────────────────────── prompts ─────────────────────────────

pub const BANNED_PHRASES: &[&str] = &[
    // cover-letter clichés
    "i am writing to express", "i am excited to apply", "i'm excited to apply", "passionate about", "in today's",
    "delve", "tapestry", "perfect fit", "dream job", "i believe i would be", "unique blend", "testament to",
    "hit the ground running", "think outside the box", "fast-paced environment",
    // weak duty language: says what the job was, not what was done
    "responsible for", "helped with", "helped to", "worked on", "assisted with", "assisted in", "assisted", "duties included",
    "tasked with", "in charge of", "familiar with", "exposure to",
    // empty self-description and jargon recruiters and screening tools discount
    "leverage", "leveraged", "leveraging", "utilize", "utilized", "utilizing", "hard worker", "hardworking", "hard-working",
    "go-getter", "go getter", "self-starter", "self starter", "detail-oriented", "detail oriented", "results-driven",
    "results-oriented", "team player", "people person", "synergy", "dynamic", "highly motivated", "self-motivated",
    "proven track record", "track record", "best of breed", "value add", "strategic thinker", "out-of-the-box",
    "excellent communication skills", "strong work ethic", "spearheaded", "various", "etc.",
];

pub const DEFAULT_GREETING: &str = "Greetings, Hiring Team,";

/// Phrases that can date a candidate to a human reader or a screening tool.
pub const AGE_SIGNALS: &[&str] = &[
    "seasoned", "decades of", "veteran", "references available upon request", "references available on request",
    "to whom it may concern", "dear sir or madam", "dear sir/madam", "objective:", "career objective",
];

static RE_LONG_TENURE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:1[5-9]|[2-9]\d)\s*\+?\s*(?:plus\s+)?years\b|\b(?:over|more than)\s+(?:1[5-9]|[2-9]\d|fifteen|twenty|thirty)\s+years\b").unwrap());

/// Whole-phrase, case-insensitive match ("dynamic" does not match "dynamics").
fn has_phrase(text_lower: &str, phrase: &str) -> bool {
    Regex::new(&format!(r"(?:^|[^\w]){}(?:$|[^\w])", regex::escape(phrase))).map(|r| r.is_match(text_lower)).unwrap_or(false)
}

/// Weak, overused and age-signalling language, flagged for review (never blocking).
pub fn style_findings(loc: &str, text: &str, out: &mut Vec<Finding>) {
    let t = text.trim();
    let lt = t.to_lowercase();
    let mut push = |msg: String| out.push(Finding { severity: "review".into(), location: loc.into(), text: t.chars().take(160).collect(), message: msg });
    for p in BANNED_PHRASES {
        if has_phrase(&lt, p) {
            push(format!("Weak or overused phrase “{p}”: recruiters and screening tools often skip past it. Say what you did and the result instead"));
        }
    }
    for p in AGE_SIGNALS {
        if has_phrase(&lt, p) {
            push(format!("“{p}” can signal age to a reader or screening tool. Consider rewording"));
        }
    }
    if let Some(m) = RE_LONG_TENURE.find(t) {
        push(format!("“{}” can signal age. Lead with recent, relevant results instead of total years", m.as_str()));
    }
}

pub fn system_prompt(kind: &str, style_notes: &str) -> String {
    let common = format!(
        "You are an experienced hiring manager and professional resume writer. You help one candidate apply for one job.\n\n\
         NON-NEGOTIABLE RULES\n\
         1. Use only the candidate facts provided. Never invent skills, tools, certifications, degrees, employers, titles, dates, metrics, numbers, team sizes or results.\n\
         2. Every piece of text you write must list in `sources` the fact IDs (e.g. \"E0.3\", \"K12\") that support it. If a fact does not support a claim, leave the claim out.\n\
         3. Numbers may appear only if they appear in a cited fact. Do not round, inflate or combine them.\n\
         4. You may rephrase and reorder to emphasise what this employer needs, and use the posting's own terminology where the facts truthfully support it. Do not stretch a fact to match a requirement it does not meet.\n\
         5. Do not mention requirements the candidate lacks, and never imply they have them.\n\
         6. Avoid stock and weak phrases, including: {}. Show the action and its result instead of naming a duty (\"Built…\", not \"Responsible for…\").\n\
         6b. Avoid language that can signal age: \"seasoned\", \"veteran\", \"decades of experience\", totals of 15 or more years, \"References available upon request\", \"Objective\", \"Dear Sir or Madam\", \"To Whom It May Concern\".\n\
         7. Write in plain, specific language. No exclamation marks.\n\
         8. Never use em dashes (—). Use commas, colons, semicolons, parentheses or separate sentences instead.\n\
         9. When you name a skill or tool, include in `sources` the matching \"Verified skill\" fact (K…) as well as the bullet that shows it.\n\
         10. Keep quantities and actions exactly as the facts state them: one workflow stays one workflow (never \"workflows\"), and a thing the candidate designed or built is described as designed or built (do not turn it into an ongoing \"automate\" or \"deliver\").\n\
         11. If a <candidate_context> block is present, it is the candidate's own background on this application (for example, that they already work for this employer). Trust it when framing the document and your notes, and do not contradict it. It is not a citable fact: never quote it in the document, and never include pay figures.\n\
         12. If a <company_language> block is present, it holds the employer's own words from its website (facts W1, W2…). Echo a few of these terms naturally so the document speaks the employer's language, but only where a candidate fact genuinely connects to them: cite that candidate fact as well as the W fact. A W fact can support a statement about the employer (its mission, values or services), never a claim about the candidate. Do not say the candidate holds a value or did work the candidate facts do not show, do not copy more than a short phrase verbatim, and do not stuff keywords (resume: at most 3 or 4 company terms in total, mostly in the headline, summary and the most relevant bullets; cover letter: 2 to 4, mostly in the opening and closing). The skills list stays verified skills only. Weave the terms in silently: never say or imply that wording comes from the employer (no \"as the company puts it\", \"in line with how X describes…\", \"echoing X's value of…\"). In the resume, do not name the employer you are applying to at all. In notes_for_candidate, name the company terms you used.\n",
        BANNED_PHRASES.join("; ")
    );
    if kind == "resume" {
        format!(
            "{common}\nTASK: tailor the candidate's resume for the posting.\n\
             - `headline`: one line, up to 12 words, built from the candidate's real roles and strengths, aligned to the posting.\n\
             - `summary`: 2–3 sentences.\n\
             - `skills`: 8–14 entries chosen ONLY from the facts labelled \"Verified skill\", copied exactly, most relevant first.\n\
             - `positions`: one entry for EVERY position ID given, in the same order. Set include=true for positions whose work supports this application; give recent relevant roles 3–6 bullets and older roles 0–2. Set include=false (no bullets) for roles that add nothing; they are still listed by title, employer and dates.\n\
             - `projects`: 0–3 entries chosen from the facts labelled \"Accomplishment\" that best support this application (most relevant first). Put its fact ID (e.g. \"A2\") in `accomplishment_id`, one sentence under 35 words in `text`, and cite it in `sources`. The app prints the project's title itself. Use an empty list if none fit.\n\
             - Bullets start with a strong past-tense verb (present tense for the current role), one idea each, under 30 words, ATS-friendly plain text.\n\
             - `notes_for_candidate`: 1–3 short sentences on what you emphasised and anything they should check.\n"
        )
    } else {
        format!(
            "{common}\nTASK: write a cover letter for the posting.\n\
             - `greeting`: \"Greetings, Hiring Team,\" unless a hiring manager is named in the posting, then \"Hello <their name>,\". Never start with \"Dear\".\n\
             - `paragraphs`: 3–4 paragraphs, 230–360 words in total. Open with why this role and employer, specifically, connect to what the candidate has actually done (no generic flattery). Highlight two or three relevant accomplishments from the facts. Close with a confident, brief next step.\n\
             - `closing`: e.g. \"Sincerely,\" (the app adds the candidate's name).\n\
             - The candidate's voice, from their own writing: {style}\n\
             - Use at most one light personal touch; keep it professional.\n\
             - `notes_for_candidate`: 1–3 short sentences on your choices.\n",
            style = if style_notes.trim().is_empty() { crate::ai::DEFAULT_STYLE } else { style_notes.trim() }
        )
    }
}

pub fn user_prompt(job: &Job, facts: &[Fact], positions: &[PositionRef]) -> String {
    let b = job.score_breakdown.clone().unwrap_or_default();
    let list = |v: &[String]| if v.is_empty() { "none".to_string() } else { v.iter().take(12).map(|x| format!("- {x}")).collect::<Vec<_>>().join("\n") };
    format!(
        "{}{}\n\nToday's date is {}. Dates on or before it are past events, not future ones.",
        candidate_context(job),
        job_and_facts(job, facts, positions, &b, &list),
        chrono::Local::now().format("%B %-d, %Y")
    )
}

/// The candidate's own notes on this application, if any (see system rule 11).
pub fn candidate_context(job: &Job) -> String {
    let mut lines = vec![];
    if job.draft.internal_candidate {
        lines.push(format!(
            "The candidate is an internal candidate: they already work at {}. Do not describe this role as a promotion or a step up unless the candidate says so below.",
            job.draft.employer
        ));
    }
    let own = job.draft.candidate_context.trim();
    if !own.is_empty() {
        lines.push(format!("In the candidate's words: {own}"));
    }
    if lines.is_empty() { String::new() } else { format!("<candidate_context>\n{}\n</candidate_context>\n\n", lines.join("\n")) }
}

fn job_and_facts(job: &Job, facts: &[Fact], positions: &[PositionRef], b: &ScoreBreakdown, list: &dyn Fn(&[String]) -> String) -> String {
    format!(
        "<job_posting>\nTitle: {}\nEmployer: {}\nLocation: {}\n\n{}\n</job_posting>\n\n\
         <fit_analysis>\nMet:\n{}\nPartially met:\n{}\nGaps (do not claim these):\n{}\n</fit_analysis>\n\n\
         <positions>\n{}\n</positions>\n\n<candidate_facts>\n{}\n</candidate_facts>",
        job.draft.title,
        job.draft.employer,
        job.draft.location_text,
        if job.draft.description.trim().is_empty() { "(No description saved; tailor to the title and employer only.)".to_string() } else { job.draft.description.chars().take(30_000).collect() },
        list(&b.satisfied),
        list(&b.partial),
        list(&b.gaps),
        positions.iter().map(|p| format!("{}: {} at {} ({})", p.id, p.exp.title, p.exp.employer, dated(&p.exp))).collect::<Vec<_>>().join("\n"),
        facts.iter().map(|f| format!("[{}] {}", f.id, f.text)).collect::<Vec<_>>().join("\n"),
    )
}

// ───────────────────────────── claim check ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// blocker | review
    pub severity: String,
    pub location: String,
    pub text: String,
    pub message: String,
}

/// Source marker for text the candidate typed themselves.
pub const USER_SOURCE: &str = "USER";

static RE_NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d[\d,]*(?:\.\d+)?").unwrap());

fn norm_num(s: &str) -> String {
    s.replace(',', "").trim_end_matches('.').to_string()
}

/// Checks one written unit against its cited facts and the whole verified profile.
pub fn check_line(loc: &str, line: &Line, facts: &[Fact], profile_evidence: &str, allowed_extra: &str, out: &mut Vec<Finding>) {
    let t = line.text.trim();
    if t.is_empty() {
        return;
    }
    let mut push = |sev: &str, msg: String| out.push(Finding { severity: sev.into(), location: loc.into(), text: t.chars().take(160).collect(), message: msg });
    // Lines the candidate wrote themselves while editing: their word, flagged for a final check.
    if line.sources.iter().any(|s| s == USER_SOURCE) {
        push("review", "Written by you — make sure it's accurate".into());
        for term in SKILL_VOCABULARY.iter().chain(CERT_TERMS.iter()) {
            if crate::documents::term_regex(term).is_match(t) && !in_profile(profile_evidence, term) {
                push("blocker", format!("Mentions “{term}”, which isn't in your verified profile — add it to your skills if it's true"));
            }
        }
        style_findings(loc, t, out);
        return;
    }
    let cited: Vec<&Fact> = line.sources.iter().filter_map(|id| facts.iter().find(|f| &f.id == id)).collect();
    let unknown: Vec<&String> = line.sources.iter().filter(|id| !facts.iter().any(|f| &f.id == *id)).collect();
    if line.sources.is_empty() {
        push("blocker", "No source cited — this line can't be traced to your verified profile".into());
    } else if !unknown.is_empty() {
        push("blocker", format!("Cites unknown source{} {}", if unknown.len() > 1 { "s" } else { "" }, unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let cited_text = cited.iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join("\n");
    let cited_nums: Vec<String> = RE_NUM.find_iter(&cited_text).map(|m| norm_num(m.as_str())).collect();
    let extra_nums: Vec<String> = RE_NUM.find_iter(allowed_extra).map(|m| norm_num(m.as_str())).collect();
    for m in RE_NUM.find_iter(t) {
        let n = norm_num(m.as_str());
        if !cited_nums.contains(&n) && !extra_nums.contains(&n) {
            push("blocker", format!("The number “{}” doesn't appear in the cited source", m.as_str()));
        }
    }
    let lower_cited = cited_text.to_lowercase();
    let company_cited = cited.iter().filter(|f| is_company_fact(&f.id)).map(|f| f.text.to_lowercase()).collect::<Vec<_>>().join("\n");
    for term in SKILL_VOCABULARY.iter().chain(CERT_TERMS.iter()) {
        if !crate::documents::term_regex(term).is_match(t) {
            continue;
        }
        // Naming the employer's own service ("your crisis management services"), backed by its website.
        if !company_cited.is_empty() && has_evidence(&company_cited, term) {
            continue;
        }
        if !in_profile(profile_evidence, term) {
            push("blocker", format!("Mentions “{term}”, which isn't in your verified profile"));
        } else if !cited.is_empty() && !has_evidence(&lower_cited, term) && !is_verified_skill(facts, term) && !stem_supported(&lower_cited, term) {
            push("review", format!("“{term}” is in your profile but not in the cited source — check the citation"));
        }
    }
    style_findings(loc, t, out);
}

/// A fact from the employer's website (company research), not from the candidate's profile.
pub fn is_company_fact(id: &str) -> bool {
    id.starts_with('W') && id[1..].chars().all(|c| c.is_ascii_digit()) && id.len() > 1
}

/// Resume lines must be about the candidate: a line backed only by the employer's website is not.
fn company_only(loc: &str, line: &Line, out: &mut Vec<Finding>) {
    if !line.sources.is_empty() && line.sources.iter().all(|s| is_company_fact(s)) {
        out.push(Finding {
            severity: "blocker".into(),
            location: loc.into(),
            text: line.text.chars().take(160).collect(),
            message: "Cites only the company's website — a resume line must be backed by your own experience".into(),
        });
    }
}

/// A resume names the employer being applied to ("…in line with how Fortive describes its way of
/// working"): unusual on a resume and it reads as copied. Fine when the candidate has worked there.
fn names_target_employer(loc: &str, text: &str, job: &Job, pos: &[PositionRef], out: &mut Vec<Finding>) {
    if text.trim().is_empty() || pos.iter().any(|p| crate::company::mentions_employer(&p.exp.employer, &job.draft.employer)) {
        return;
    }
    if crate::company::mentions_employer(text, &job.draft.employer) {
        out.push(Finding {
            severity: "review".into(),
            location: loc.into(),
            text: text.chars().take(160).collect(),
            message: format!("Names {}, the employer you're applying to — resumes usually don't, and it can read as copied from their website", job.draft.employer),
        });
    }
}

/// The profile shows the term, in any word form: "onboarded" in a bullet supports "Onboarding".
fn in_profile(profile_evidence: &str, term: &str) -> bool {
    has_evidence(profile_evidence, term) || stem_supported(&profile_evidence.to_lowercase(), term)
}

/// The term is one of the candidate's verified skills (a true claim even if a bullet was cited instead).
fn is_verified_skill(facts: &[Fact], term: &str) -> bool {
    let t = term.to_lowercase();
    facts.iter().any(|f| f.text.strip_prefix("Verified skill: ").map(|n| n.to_lowercase().split(" (").next() == Some(t.as_str())).unwrap_or(false))
}

/// The cited text supports the term through a word form ("validate" for "Data Validation").
fn stem_supported(cited_lower: &str, term: &str) -> bool {
    let words: Vec<String> = term.to_lowercase().split_whitespace().map(String::from).collect();
    !words.is_empty()
        && words.iter().all(|w| {
            let stem: String = w.chars().take(5.max(w.chars().count().saturating_sub(3))).collect();
            stem.len() >= 3 && cited_lower.contains(&stem)
        })
}

pub fn check_resume(c: &ResumeContent, facts: &[Fact], skills: &[Skill], profile_evidence: &str, job: &Job, pos: &[PositionRef]) -> Vec<Finding> {
    let mut out = vec![];
    let extra = format!("{} {}", job.draft.title, job.draft.employer);
    check_line("Summary", &c.summary, facts, profile_evidence, &extra, &mut out);
    company_only("Summary", &c.summary, &mut out);
    names_target_employer("Summary", &c.summary.text, job, pos, &mut out);
    names_target_employer("Headline", &c.headline, job, pos, &mut out);
    if !c.headline.trim().is_empty() {
        // The headline is short and uncited: check it for unverified tools and numbers only.
        let mut tmp = vec![];
        check_line("Headline", &Line { text: c.headline.clone(), sources: vec!["H1".into(), "S1".into()] }, facts, profile_evidence, &extra, &mut tmp);
        out.extend(tmp.into_iter().filter(|f| !f.message.starts_with("Cites unknown") && !f.message.contains("cited source — check")));
    }
    for s in &c.skills {
        if !skills.iter().any(|k| k.name.eq_ignore_ascii_case(s.trim())) {
            out.push(Finding { severity: "blocker".into(), location: "Skills".into(), text: s.clone(), message: "Not one of your verified skills".into() });
        }
    }
    check_projects(c, facts, profile_evidence, &extra, &mut out);
    for p in &c.positions {
        let label = pos.iter().find(|x| x.id == p.position_id).map(|x| format!("{} — {}", x.exp.title, x.exp.employer));
        let Some(label) = label else {
            out.push(Finding { severity: "blocker".into(), location: "Experience".into(), text: p.position_id.clone(), message: "Unknown position — it will be ignored".into() });
            continue;
        };
        for (i, b) in p.bullets.iter().enumerate() {
            check_line(&format!("{label}, bullet {}", i + 1), b, facts, profile_evidence, &extra, &mut out);
            company_only(&format!("{label}, bullet {}", i + 1), b, &mut out);
            names_target_employer(&format!("{label}, bullet {}", i + 1), &b.text, job, pos, &mut out);
        }
    }
    out
}

fn check_projects(c: &ResumeContent, facts: &[Fact], profile_evidence: &str, extra: &str, out: &mut Vec<Finding>) {
    for (i, pr) in c.projects.iter().enumerate() {
        let id = pr.accomplishment_id.trim();
        if !id.starts_with('A') || !facts.iter().any(|f| f.id == id) {
            out.push(Finding { severity: "blocker".into(), location: "Selected Projects".into(), text: id.to_string(), message: "Unknown project — it will be left out".into() });
            continue;
        }
        let mut sources = pr.sources.clone();
        if !sources.iter().any(|s| s == id) && !sources.iter().any(|s| s == USER_SOURCE) {
            sources.push(id.to_string());
        }
        check_line(&format!("Project {}", i + 1), &Line { text: pr.text.clone(), sources }, facts, profile_evidence, extra, out);
    }
}

pub fn check_cover_letter(c: &CoverLetterContent, facts: &[Fact], profile_evidence: &str, job: &Job) -> Vec<Finding> {
    let mut out = vec![];
    let extra = format!("{} {} {}", job.draft.title, job.draft.employer, job.draft.location_text);
    let g = c.greeting.trim().to_lowercase();
    if g.starts_with("dear") || g.starts_with("to whom") {
        out.push(Finding { severity: "review".into(), location: "Greeting".into(), text: c.greeting.trim().into(), message: format!("“{}” can read as dated. Consider “{DEFAULT_GREETING}”", c.greeting.trim()) });
    }
    for (i, p) in c.paragraphs.iter().enumerate() {
        check_line(&format!("Paragraph {}", i + 1), p, facts, profile_evidence, &extra, &mut out);
    }
    out
}

// ───────────────────────────── rendering ─────────────────────────────

/// Layout-neutral document; rendered to DOCX, PDF and plain text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Block {
    Name(String),
    Contact(String),
    Headline(String),
    Heading(String),
    Role { title: String, meta: String },
    Para(String),
    Bullet(String),
    Project { title: String, text: String },
    Gap,
}

fn contact_line(b: &ProfileBasics) -> String {
    [b.location.as_str(), b.email.as_str(), b.phone.as_str(), b.linkedin_url.trim_start_matches("https://").trim_start_matches("www.")]
        .iter()
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("  |  ")
}

pub fn render_resume(p: &ProfileBundle, c: &ResumeContent) -> Vec<Block> {
    let mut b = vec![Block::Name(p.basics.full_name.clone()), Block::Contact(contact_line(&p.basics))];
    if !c.headline.trim().is_empty() {
        b.push(Block::Headline(c.headline.trim().to_string()));
    }
    if !c.summary.text.trim().is_empty() {
        b.push(Block::Heading("Professional Summary".into()));
        b.push(Block::Para(c.summary.text.trim().to_string()));
    }
    let skills: Vec<String> = c.skills.iter().filter_map(|s| p.skills.iter().find(|k| k.name.eq_ignore_ascii_case(s.trim())).map(|k| k.name.clone())).collect();
    if !skills.is_empty() {
        b.push(Block::Heading("Core Skills".into()));
        b.push(Block::Para(skills.join("  •  ")));
    }
    let pos = positions(p);
    let content_for = |id: &str| c.positions.iter().find(|x| x.position_id == id);
    let (main, other): (Vec<&PositionRef>, Vec<&PositionRef>) = pos.iter().partition(|x| content_for(&x.id).map(|c| c.include && !c.bullets.is_empty()).unwrap_or(false));
    if !main.is_empty() {
        b.push(Block::Heading("Professional Experience".into()));
        for x in main {
            let meta = [x.exp.employer.as_str(), x.exp.location.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(", ");
            b.push(Block::Role { title: x.exp.title.clone(), meta: match shown_dates(&x.exp) { Some(d) => format!("{meta}  |  {d}"), None => meta.clone() } });
            for bl in &content_for(&x.id).unwrap().bullets {
                if !bl.text.trim().is_empty() {
                    b.push(Block::Bullet(bl.text.trim().to_string()));
                }
            }
        }
    }
    let projects: Vec<(String, String)> = c
        .projects
        .iter()
        .filter(|x| !x.text.trim().is_empty())
        .filter_map(|x| {
            let i: usize = x.accomplishment_id.trim().strip_prefix('A')?.parse().ok()?;
            p.accomplishments.get(i).map(|a| (a.title.clone(), x.text.trim().to_string()))
        })
        .take(3)
        .collect();
    if !projects.is_empty() {
        b.push(Block::Heading("Selected Projects".into()));
        for (title, text) in projects {
            b.push(Block::Project { title, text });
        }
    }
    if !other.is_empty() {
        b.push(Block::Heading("Additional Experience".into()));
        for x in other {
            b.push(Block::Para(match shown_dates(&x.exp) { Some(d) => format!("{}, {}, {d}", x.exp.title, x.exp.employer), None => format!("{}, {}", x.exp.title, x.exp.employer) }));
        }
    }
    if !p.education.is_empty() {
        b.push(Block::Heading("Education".into()));
        for e in &p.education {
            let mut s = [e.degree.as_str(), e.field.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(", ");
            if !e.minor.is_empty() {
                s.push_str(&format!("; minor in {}", e.minor));
            }
            b.push(Block::Para(format!("{}{}{}", s, if s.is_empty() { "" } else { ", " }, e.institution)));
        }
    }
    if !p.certifications.is_empty() {
        b.push(Block::Heading("Certifications".into()));
        // Several certificates from one issuer read better as one line than as a long list.
        let mut issuers: Vec<String> = vec![];
        for cert in &p.certifications {
            if !issuers.contains(&cert.issuer) {
                issuers.push(cert.issuer.clone());
            }
        }
        for issuer in issuers {
            let group: Vec<&Certification> = p.certifications.iter().filter(|c| c.issuer == issuer).collect();
            if group.len() >= 3 && !issuer.is_empty() {
                let mut years: Vec<&str> = group.iter().filter_map(|c| c.date_earned.get(..4)).collect();
                years.sort();
                years.dedup();
                let when = match (years.first(), years.last()) {
                    (Some(a), Some(z)) if a != z => format!(" ({a}–{z})"),
                    (Some(a), _) => format!(" ({a})"),
                    _ => String::new(),
                };
                let names = group.iter().map(|c| c.name.trim_end_matches(" - Self-Paced").trim_end_matches(" - Self Paced").to_string()).collect::<Vec<_>>().join("; ");
                b.push(Block::Bullet(format!("{issuer}{when}: {names}")));
            } else {
                for cert in group {
                    let date = if cert.date_earned.is_empty() { String::new() } else { month_name(&cert.date_earned) };
                    let detail = [cert.issuer.as_str(), date.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(", ");
                    b.push(Block::Bullet(if detail.is_empty() { cert.name.clone() } else { format!("{}, {}", cert.name, detail) }));
                }
            }
        }
    }
    b
}

pub fn render_cover_letter(p: &ProfileBundle, job: &Job, c: &CoverLetterContent, date: &str) -> Vec<Block> {
    let mut b = vec![Block::Name(p.basics.full_name.clone()), Block::Contact(contact_line(&p.basics)), Block::Gap, Block::Para(date.to_string()), Block::Gap];
    b.push(Block::Para(format!("Hiring Team\n{}", job.draft.employer)));
    b.push(Block::Para(format!("Re: {}", job.draft.title)));
    b.push(Block::Gap);
    b.push(Block::Para(if c.greeting.trim().is_empty() { DEFAULT_GREETING.into() } else { c.greeting.trim().to_string() }));
    for para in &c.paragraphs {
        if !para.text.trim().is_empty() {
            b.push(Block::Para(para.text.trim().to_string()));
        }
    }
    b.push(Block::Para(if c.closing.trim().is_empty() { "Sincerely,".into() } else { c.closing.trim().to_string() }));
    b.push(Block::Para(p.basics.full_name.clone()));
    b
}

static RE_EM_DASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*—\s*").unwrap());

/// Replaces em dashes with a comma (the candidate prefers not to use them).
pub fn clean_dashes(s: &str) -> String {
    let t = RE_EM_DASH.replace_all(s.trim(), ", ").to_string();
    t.replace(",,", ",").replace(", .", ".").replace(", ,", ",").trim_start_matches(", ").to_string()
}

pub fn clean_resume(mut r: ResumeContent) -> ResumeContent {
    r.headline = clean_dashes(&r.headline);
    r.summary.text = clean_dashes(&r.summary.text);
    for x in r.projects.iter_mut() {
        x.text = clean_dashes(&x.text);
    }
    for p in r.positions.iter_mut() {
        for b in p.bullets.iter_mut() {
            b.text = clean_dashes(&b.text);
        }
    }
    r
}

pub fn clean_letter(mut l: CoverLetterContent) -> CoverLetterContent {
    l.greeting = clean_dashes(&l.greeting);
    l.closing = clean_dashes(&l.closing);
    for p in l.paragraphs.iter_mut() {
        p.text = clean_dashes(&p.text);
    }
    l
}

pub fn plain_text(blocks: &[Block]) -> String {
    let mut s = String::new();
    for b in blocks {
        match b {
            Block::Name(t) | Block::Contact(t) | Block::Headline(t) | Block::Para(t) => s.push_str(&format!("{t}\n")),
            Block::Heading(t) => s.push_str(&format!("\n{}\n", t.to_uppercase())),
            Block::Role { title, meta } => s.push_str(&format!("\n{title}\n{meta}\n")),
            Block::Bullet(t) => s.push_str(&format!("• {t}\n")),
            Block::Project { title, text } => s.push_str(&format!("{title}: {text}\n")),
            Block::Gap => s.push('\n'),
        }
    }
    s.trim().to_string()
}

pub fn word_count(c: &CoverLetterContent) -> usize {
    c.paragraphs.iter().map(|p| p.text.split_whitespace().count()).sum()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn profile() -> ProfileBundle {
        ProfileBundle {
            basics: ProfileBasics {
                full_name: "Jordan Sample".into(),
                email: "jordan@example.com".into(),
                location: "Milwaukee, Wisconsin".into(),
                summary: "Operations specialist with 6+ years in crisis management operations.".into(),
                ..Default::default()
            },
            experience: vec![
                Experience {
                    employer: "Contoso".into(),
                    title: "Business Operations Specialist".into(),
                    start_date: "2023-11".into(),
                    is_current: true,
                    achievements: "Designed an Excel outreach workflow for a 6,000-record activation supporting 15 agents\nBuilt Smartsheet dashboards and testing trackers".into(),
                    ..Default::default()
                },
                Experience { employer: "Fabrikam Hotels".into(), title: "Guest Service".into(), start_date: "2002".into(), end_date: "2020".into(), achievements: "Earned customer-service awards".into(), ..Default::default() },
            ],
            education: vec![Education { institution: "State University".into(), degree: "Bachelor's degree".into(), field: "Public Relations".into(), minor: "Marketing".into(), ..Default::default() }],
            skills: vec![Skill { name: "Smartsheet".into(), ..Default::default() }, Skill { name: "Excel".into(), ..Default::default() }],
            certifications: vec![Certification { name: "Smartsheet Builder".into(), issuer: "Smartsheet University".into(), date_earned: "2025-04".into(), ..Default::default() }],
            accomplishments: vec![Accomplishment { title: "Call center template".into(), description: "Created a Microsoft Word call center template with escalation triggers and call flows".into(), ..Default::default() }],
            documents: vec![],
        }
    }

    #[test]
    fn candidate_context_only_when_given() {
        let mut j = job();
        let facts = facts(&profile());
        assert!(!user_prompt(&j, &facts, &[]).contains("<candidate_context>"));
        j.draft.internal_candidate = true;
        j.draft.candidate_context = "Lateral move; same pay band.".into();
        let u = user_prompt(&j, &facts, &[]);
        assert!(u.contains("Today's date is ") && u.contains("past events"), "the model must know today's date");
        assert!(u.starts_with("<candidate_context>"));
        assert!(u.contains("internal candidate") && u.contains("Lateral move; same pay band."));
        assert!(system_prompt("resume", "").contains("11. If a <candidate_context>"));
    }

    pub fn job() -> Job {
        Job {
            id: 1,
            draft: JobDraft { title: "Analytics Coordinator III".into(), employer: "HSAG".into(), description: "Requirements\n• Excel".into(), ..Default::default() },
            salary_flag: "meets".into(),
            review_flags: vec![],
            excluded_reasons: vec![],
            expired: false,
            status: "Discovered".into(),
            score: Some(80),
            score_breakdown: None,
            assessment: None,
            notes: String::new(),
            submitted_at: None,
            confirmation_ref: None,
            confirmation_method: None,
            is_sample: false,
            discovered_at: "2026-11-01 09:00:00".into(),
            updated_at: "2026-11-01 09:00:00".into(),
        }
    }

    fn evidence(p: &ProfileBundle) -> String {
        crate::scoring::profile_facts(p, "", chrono::NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()).evidence
    }

    #[test]
    fn facts_have_ids_and_no_contact_details() {
        let p = profile();
        let f = facts(&p);
        assert!(f.iter().any(|x| x.id == "E0.0" && x.text.contains("6,000-record")));
        assert!(f.iter().any(|x| x.id == "P1" && x.text.contains("2002 – 2020")));
        assert!(f.iter().any(|x| x.id == "K1" && x.text.contains("Excel")));
        assert!(!f.iter().any(|x| x.text.contains("jordan@example.com")), "contact details are never sent");
        let u = user_prompt(&job(), &f, &positions(&p));
        assert!(u.contains("[E0.1] [Business Operations Specialist, Contoso] Built Smartsheet dashboards"));
        assert!(u.contains("P0: Business Operations Specialist at Contoso (Nov 2023 – Present)"));
    }

    #[test]
    fn company_language_supports_statements_about_the_employer_only() {
        let p = profile();
        let mut f = facts(&p);
        let ev = evidence(&p);
        f.push(Fact { id: "W1".into(), text: "Phrases the employer uses about itself: whole health; Zendesk support desk".into() });
        assert!(is_company_fact("W1") && !is_company_fact("WX") && !is_company_fact("W"));
        // About the employer, backed by its website: the tool-vocabulary check doesn't block it.
        let mut out = vec![];
        check_line("p1", &Line { text: "Your Zendesk support desk serves members well".into(), sources: vec!["W1".into()] }, &f, &ev, "", &mut out);
        assert!(out.is_empty(), "{out:?}");
        // Claiming the skill for the candidate, backed only by the website, is still blocked.
        let mut out = vec![];
        check_line("p2", &Line { text: "Ran the Zendesk support desk for 15 agents".into(), sources: vec!["E0.0".into()] }, &f, &ev, "", &mut out);
        assert!(out.iter().any(|x| x.severity == "blocker" && x.message.contains("Zendesk")), "{out:?}");
        // A resume line backed only by the employer's website is blocked.
        let mut r: ResumeContent = serde_json::from_value(serde_json::json!({
            "headline": "", "summary": {"text": "Committed to whole health", "sources": ["W1"]}, "skills": [], "positions": [], "projects": [], "notes_for_candidate": ""
        })).unwrap();
        let out = check_resume(&r, &f, &p.skills, &ev, &job(), &positions(&p));
        assert!(out.iter().any(|x| x.message.starts_with("Cites only the company's website")), "{out:?}");
        r.summary.sources.push("E0.0".into());
        let out = check_resume(&r, &f, &p.skills, &ev, &job(), &positions(&p));
        assert!(!out.iter().any(|x| x.message.starts_with("Cites only")), "{out:?}");
        assert!(system_prompt("cover_letter", "").contains("12. If a <company_language>"));
        assert!(system_prompt("resume", "").contains("never say or imply that wording comes from the employer"));
        // A word form in the profile ("onboarded") supports the vocabulary term ("Onboarding").
        assert!(in_profile("hired, trained and onboarded 1099 contractors", "Onboarding"));
        assert!(!in_profile("hired and trained 1099 contractors", "Onboarding"));
        // Naming the target employer in a resume is flagged for review, unless the candidate worked there.
        let mut j = job();
        j.draft.employer = "Fortive".into();
        let r: ResumeContent = serde_json::from_value(serde_json::json!({
            "headline": "", "summary": {"text": "Applies continuous improvement, in line with how Fortive describes its way of working", "sources": ["S1"]},
            "skills": [], "positions": [], "projects": [], "notes_for_candidate": ""
        })).unwrap();
        let out = check_resume(&r, &f, &p.skills, &ev, &j, &positions(&p));
        assert!(out.iter().any(|x| x.severity == "review" && x.message.starts_with("Names Fortive")), "{out:?}");
        j.draft.employer = p.experience[0].employer.clone();
        let mut r2 = r.clone();
        r2.summary.text = format!("Six years at {}", p.experience[0].employer);
        let out = check_resume(&r2, &f, &p.skills, &ev, &j, &positions(&p));
        assert!(!out.iter().any(|x| x.message.starts_with("Names ")), "internal candidates may name their employer: {out:?}");
    }

    #[test]
    fn dates_are_left_off_roles_that_ended_15_or_more_years_ago() {
        let role = |start: &str, end: &str, current: bool| Experience { id: None, employer: "E".into(), title: "T".into(), location: String::new(),
            start_date: start.into(), end_date: end.into(), is_current: current, achievements: String::new(), sort_order: 0 };
        assert_eq!(shown_dates_in(&role("1992", "1996", false), 2026), None);
        assert_eq!(shown_dates_in(&role("1994", "2004", false), 2026), None);
        assert_eq!(shown_dates_in(&role("2005", "2011", false), 2026), None, "ended exactly 15 years ago");
        assert_eq!(shown_dates_in(&role("2002", "2020", false), 2026).as_deref(), Some("2002 – 2020"));
        assert_eq!(shown_dates_in(&role("2005", "2012", false), 2026).as_deref(), Some("2005 – 2012"));
        assert!(shown_dates_in(&role("1990-05", "", true), 2026).unwrap().ends_with("Present"), "current roles keep their dates");
        // An old role's dates never reach the model either, so it can't write them into a letter.
        let mut p = profile();
        p.experience.push(role("1992", "1996", false));
        let f = facts(&p);
        let old = f.iter().find(|x| x.text.starts_with("Position: T at E")).unwrap();
        assert!(old.text.contains("earlier career; dates not shown") && !old.text.contains("1992"), "{}", old.text);
        let t = plain_text(&render_resume(&p, &ResumeContent { headline: String::new(), summary: Line { text: String::new(), sources: vec![] },
            skills: vec![], positions: vec![], projects: vec![], notes_for_candidate: String::new() }));
        assert!(!t.contains("1992") && !t.contains("1996") && t.contains("T, E"), "{t}");
    }

    #[test]
    fn flags_weak_overused_and_age_signalling_language() {
        let flagged = |t: &str| { let mut o = vec![]; style_findings("x", t, &mut o); o };
        for t in ["Responsible for the help desk", "Helped with onboarding", "Worked on the dashboard", "Assisted the team",
                  "Leverage data to win", "A hard worker and a go getter", "Results-driven self-starter", "Utilized Excel daily"] {
            let o = flagged(t);
            assert!(!o.is_empty() && o.iter().all(|f| f.severity == "review"), "{t}: {o:?}");
        }
        assert!(flagged("Seasoned operations leader").iter().any(|f| f.message.contains("signal age")));
        assert!(flagged("With 25+ years of experience").iter().any(|f| f.message.contains("25+ years")));
        assert!(flagged("over twenty years in hospitality").iter().any(|f| f.message.contains("signal age")));
        // Whole words only, and ordinary recent tenure is fine.
        assert!(flagged("Built team dynamics dashboards over 6+ years").is_empty(), "{:?}", flagged("Built team dynamics dashboards over 6+ years"));
        assert!(flagged("Built a Smartsheet payment workflow that cut processing to 15 minutes").is_empty());
        // Dated greetings are flagged on cover letters.
        let p = profile();
        let mut cl = CoverLetterContent { greeting: "Dear Hiring Team,".into(), paragraphs: vec![], closing: String::new(), notes_for_candidate: String::new() };
        assert!(check_cover_letter(&cl, &facts(&p), "", &job()).iter().any(|f| f.location == "Greeting"));
        cl.greeting = DEFAULT_GREETING.into();
        assert!(check_cover_letter(&cl, &facts(&p), "", &job()).is_empty());
        assert!(system_prompt("cover_letter", "").contains("Greetings, Hiring Team,") && system_prompt("resume", "").contains("signal age"));
    }

    #[test]
    fn claim_check_blocks_invented_content() {
        let p = profile();
        let f = facts(&p);
        let ev = evidence(&p);
        let j = job();
        let good = Line { text: "Designed an Excel outreach workflow for a 6,000-record activation supporting 15 agents".into(), sources: vec!["E0.0".into()] };
        let mut out = vec![];
        check_line("b1", &good, &f, &ev, "", &mut out);
        assert!(out.is_empty(), "{out:?}");

        let mut out = vec![];
        check_line("b2", &Line { text: "Cut reporting time by 40% using SQL and Tableau".into(), sources: vec!["E0.1".into()] }, &f, &ev, "", &mut out);
        assert!(out.iter().any(|x| x.message.contains("“40”")));
        assert!(out.iter().any(|x| x.message.contains("SQL")));
        assert!(out.iter().any(|x| x.message.contains("Tableau")));
        assert!(out.iter().all(|x| x.severity == "blocker"));

        let mut out = vec![];
        check_line("b3", &Line { text: "Led a team of 20".into(), sources: vec![] }, &f, &ev, "", &mut out);
        assert!(out.iter().any(|x| x.message.starts_with("No source")));

        let mut out = vec![];
        check_line("b4", &Line { text: "Built Excel trackers".into(), sources: vec!["E9.9".into()] }, &f, &ev, "", &mut out);
        assert!(out.iter().any(|x| x.message.contains("unknown source")));

        let mut out = vec![];
        check_line("u1", &Line { text: "Mentored 3 new coordinators".into(), sources: vec![USER_SOURCE.into()] }, &f, &ev, "", &mut out);
        assert_eq!(out.len(), 1, "user-written lines are flagged for review, not blocked: {out:?}");
        let mut out = vec![];
        check_line("u2", &Line { text: "Built Tableau reports".into(), sources: vec![USER_SOURCE.into()] }, &f, &ev, "", &mut out);
        assert!(out.iter().any(|x| x.severity == "blocker" && x.message.contains("Tableau")));

        // A verified skill named in a line that cites a related bullet is not flagged
        let mut out = vec![];
        check_line("s1", &Line { text: "Builds Excel trackers in Smartsheet".into(), sources: vec!["E0.0".into()] }, &f, &ev, "", &mut out);
        assert!(out.is_empty(), "{out:?}");
        assert!(stem_supported("clean, standardize, validate, and reconcile data", "Data Validation"));
        assert!(!stem_supported("built dashboards", "Data Validation"));

        let mut out = vec![];
        check_line("b5", &Line { text: "I am passionate about dashboards in Smartsheet".into(), sources: vec!["E0.1".into()] }, &f, &ev, "", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].severity, "review");

        // Numbers from the job title are allowed (e.g. "III" is not a number, "2026" from a posting would be)
        let r = ResumeContent {
            headline: "Operations specialist".into(),
            summary: Line { text: "Operations specialist with 6+ years in crisis management operations.".into(), sources: vec!["S1".into()] },
            skills: vec!["Excel".into(), "Python".into()],
            positions: vec![PositionContent { position_id: "P7".into(), include: true, bullets: vec![] }],
            projects: vec![],
            notes_for_candidate: String::new(),
        };
        let findings = check_resume(&r, &f, &p.skills, &ev, &j, &positions(&p));
        assert!(findings.iter().any(|x| x.location == "Skills" && x.text == "Python"));
        assert!(findings.iter().any(|x| x.message.starts_with("Unknown position")));
        assert!(!findings.iter().any(|x| x.location == "Summary"), "{findings:?}");
    }

    #[test]
    fn rendering_uses_profile_facts_for_titles_dates_and_education() {
        let p = profile();
        let r = ResumeContent {
            headline: "Business operations and Smartsheet automation".into(),
            summary: Line { text: "Operations specialist.".into(), sources: vec!["S1".into()] },
            skills: vec!["smartsheet".into(), "Excel".into(), "Made Up".into()],
            positions: vec![
                PositionContent { position_id: "P0".into(), include: true, bullets: vec![Line { text: "Built Smartsheet dashboards".into(), sources: vec!["E0.1".into()] }] },
                PositionContent { position_id: "P1".into(), include: false, bullets: vec![] },
            ],
            projects: vec![
                ProjectContent { accomplishment_id: "A0".into(), text: "Word call center template with escalation triggers and call flows.".into(), sources: vec![] },
                ProjectContent { accomplishment_id: "A9".into(), text: "Ghost".into(), sources: vec![] },
            ],
            notes_for_candidate: String::new(),
        };
        let text = plain_text(&render_resume(&p, &r));
        assert!(text.starts_with("Jordan Sample\nMilwaukee, Wisconsin  |  jordan@example.com"));
        assert!(text.contains("Business Operations Specialist\nContoso  |  Nov 2023 – Present\n• Built Smartsheet dashboards"));
        assert!(text.contains("ADDITIONAL EXPERIENCE\nGuest Service, Fabrikam Hotels, 2002 – 2020"));
        assert!(text.contains("SELECTED PROJECTS\nCall center template: Word call center template with escalation triggers and call flows."), "{text}");
        assert!(!text.contains("Ghost"), "unknown projects are never printed");
        let findings = check_resume(&r, &facts(&p), &p.skills, &evidence(&p), &job(), &positions(&p));
        assert!(findings.iter().any(|f| f.location == "Selected Projects" && f.severity == "blocker"));
        assert!(!findings.iter().any(|f| f.location == "Project 1"), "project cites its accomplishment automatically: {findings:?}");
        assert!(text.contains("Smartsheet  •  Excel"), "verified names with their own casing; unverified dropped");
        assert!(!text.contains("Made Up"));
        assert!(text.contains("Bachelor's degree, Public Relations; minor in Marketing, State University"));
        assert!(text.contains("• Smartsheet Builder, Smartsheet University, Apr 2025"), "{text}");
        assert!(!text.contains('—'), "no em dashes anywhere in the rendered resume");
        assert_eq!(clean_dashes("Built dashboards — fast — for leaders"), "Built dashboards, fast, for leaders");
        assert_eq!(clean_dashes("Results—not noise."), "Results, not noise.");
        let mut many = p.clone();
        many.certifications = (0..4)
            .map(|i| Certification { name: format!("Course {i} - Self-Paced"), issuer: "Smartsheet University".into(), date_earned: format!("202{}-04", 4 + i % 2), ..Default::default() })
            .collect();
        let t2 = plain_text(&render_resume(&many, &r));
        assert!(t2.contains("• Smartsheet University (2024–2025): Course 0; Course 1; Course 2; Course 3"), "{t2}");

        let cl = CoverLetterContent { greeting: String::new(), paragraphs: vec![Line { text: "One two three.".into(), sources: vec!["S1".into()] }], closing: String::new(), notes_for_candidate: String::new() };
        let t = plain_text(&render_cover_letter(&p, &job(), &cl, "November 4, 2026"));
        assert!(t.contains("Hiring Team\nHSAG\nRe: Analytics Coordinator III"));
        assert!(t.contains("Greetings, Hiring Team,") && !t.contains("Dear"));
        assert!(t.ends_with("Sincerely,\nJordan Sample"));
        assert_eq!(word_count(&cl), 3);
    }

    #[test]
    fn schemas_are_strict() {
        for s in [resume_schema(), cover_letter_schema()] {
            assert_eq!(s["additionalProperties"], false);
            assert!(s["required"].as_array().unwrap().len() >= 4);
        }
        assert!(system_prompt("cover_letter", "Warm storyteller").contains("Warm storyteller"));
        assert!(system_prompt("resume", "").contains("Verified skill"));
    }
}
