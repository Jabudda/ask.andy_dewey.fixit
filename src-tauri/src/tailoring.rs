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
        f.push(Fact { id: format!("P{i}"), text: format!("Position: {} at {} ({})", e.title, e.employer, date_range(e)) });
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
    pub notes_for_candidate: String,
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
        "notes_for_candidate": {"type": "string"}
    }, "required": ["headline", "summary", "skills", "positions", "notes_for_candidate"], "additionalProperties": false})
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
    "i am writing to express", "i am excited to apply", "i'm excited to apply", "passionate about", "dynamic", "synergy",
    "results-driven", "go-getter", "team player", "think outside the box", "fast-paced environment", "in today's",
    "delve", "tapestry", "leverage my", "proven track record", "hit the ground running", "perfect fit", "dream job",
    "i believe i would be", "unique blend", "spearheaded", "testament to",
];

pub fn system_prompt(kind: &str, style_notes: &str) -> String {
    let common = format!(
        "You are an experienced hiring manager and professional resume writer. You help one candidate apply for one job.\n\n\
         NON-NEGOTIABLE RULES\n\
         1. Use only the candidate facts provided. Never invent skills, tools, certifications, degrees, employers, titles, dates, metrics, numbers, team sizes or results.\n\
         2. Every piece of text you write must list in `sources` the fact IDs (e.g. \"E0.3\", \"K12\") that support it. If a fact does not support a claim, leave the claim out.\n\
         3. Numbers may appear only if they appear in a cited fact. Do not round, inflate or combine them.\n\
         4. You may rephrase and reorder to emphasise what this employer needs, and use the posting's own terminology where the facts truthfully support it. Do not stretch a fact to match a requirement it does not meet.\n\
         5. Do not mention requirements the candidate lacks, and never imply they have them.\n\
         6. Avoid stock phrases, including: {}.\n\
         7. Write in plain, specific language. No exclamation marks.\n\
         8. Never use em dashes (—). Use commas, colons, semicolons, parentheses or separate sentences instead.\n\
         9. When you name a skill or tool, include in `sources` the matching \"Verified skill\" fact (K…) as well as the bullet that shows it.\n",
        BANNED_PHRASES.join("; ")
    );
    if kind == "resume" {
        format!(
            "{common}\nTASK: tailor the candidate's resume for the posting.\n\
             - `headline`: one line, up to 12 words, built from the candidate's real roles and strengths, aligned to the posting.\n\
             - `summary`: 2–3 sentences.\n\
             - `skills`: 8–14 entries chosen ONLY from the facts labelled \"Verified skill\", copied exactly, most relevant first.\n\
             - `positions`: one entry for EVERY position ID given, in the same order. Set include=true for positions whose work supports this application; give recent relevant roles 3–6 bullets and older roles 0–2. Set include=false (no bullets) for roles that add nothing; they are still listed by title, employer and dates.\n\
             - Bullets start with a strong past-tense verb (present tense for the current role), one idea each, under 30 words, ATS-friendly plain text.\n\
             - `notes_for_candidate`: 1–3 short sentences on what you emphasised and anything they should check.\n"
        )
    } else {
        format!(
            "{common}\nTASK: write a cover letter for the posting.\n\
             - `greeting`: \"Dear Hiring Team,\" unless a hiring manager is named in the posting.\n\
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
        positions.iter().map(|p| format!("{}: {} at {} ({})", p.id, p.exp.title, p.exp.employer, date_range(&p.exp))).collect::<Vec<_>>().join("\n"),
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
            if crate::documents::term_regex(term).is_match(t) && !has_evidence(profile_evidence, term) {
                push("blocker", format!("Mentions “{term}”, which isn't in your verified profile — add it to your skills if it's true"));
            }
        }
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
    for term in SKILL_VOCABULARY.iter().chain(CERT_TERMS.iter()) {
        if !crate::documents::term_regex(term).is_match(t) {
            continue;
        }
        if !has_evidence(profile_evidence, term) {
            push("blocker", format!("Mentions “{term}”, which isn't in your verified profile"));
        } else if !cited.is_empty() && !has_evidence(&lower_cited, term) && !is_verified_skill(facts, term) && !stem_supported(&lower_cited, term) {
            push("review", format!("“{term}” is in your profile but not in the cited source — check the citation"));
        }
    }
    let lt = t.to_lowercase();
    for p in BANNED_PHRASES {
        if lt.contains(p) {
            push("review", format!("Stock phrase “{p}” — consider rewording"));
        }
    }
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
    for p in &c.positions {
        let label = pos.iter().find(|x| x.id == p.position_id).map(|x| format!("{} — {}", x.exp.title, x.exp.employer));
        let Some(label) = label else {
            out.push(Finding { severity: "blocker".into(), location: "Experience".into(), text: p.position_id.clone(), message: "Unknown position — it will be ignored".into() });
            continue;
        };
        for (i, b) in p.bullets.iter().enumerate() {
            check_line(&format!("{label}, bullet {}", i + 1), b, facts, profile_evidence, &extra, &mut out);
        }
    }
    out
}

pub fn check_cover_letter(c: &CoverLetterContent, facts: &[Fact], profile_evidence: &str, job: &Job) -> Vec<Finding> {
    let mut out = vec![];
    let extra = format!("{} {} {}", job.draft.title, job.draft.employer, job.draft.location_text);
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
            b.push(Block::Role { title: x.exp.title.clone(), meta: format!("{meta}  |  {}", date_range(&x.exp)) });
            for bl in &content_for(&x.id).unwrap().bullets {
                if !bl.text.trim().is_empty() {
                    b.push(Block::Bullet(bl.text.trim().to_string()));
                }
            }
        }
    }
    if !other.is_empty() {
        b.push(Block::Heading("Additional Experience".into()));
        for x in other {
            b.push(Block::Para(format!("{}, {}, {}", x.exp.title, x.exp.employer, date_range(&x.exp))));
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
    b.push(Block::Para(if c.greeting.trim().is_empty() { "Dear Hiring Team,".into() } else { c.greeting.trim().to_string() }));
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
            accomplishments: vec![],
            documents: vec![],
        }
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
            notes_for_candidate: String::new(),
        };
        let text = plain_text(&render_resume(&p, &r));
        assert!(text.starts_with("Jordan Sample\nMilwaukee, Wisconsin  |  jordan@example.com"));
        assert!(text.contains("Business Operations Specialist\nContoso  |  Nov 2023 – Present\n• Built Smartsheet dashboards"));
        assert!(text.contains("ADDITIONAL EXPERIENCE\nGuest Service, Fabrikam Hotels, 2002 – 2020"));
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
        assert!(t.contains("Dear Hiring Team,"));
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
