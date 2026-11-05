//! Generation pipeline: prepare (what will be sent) → Claude → claim check → DOCX + PDF →
//! versioned draft → user edits → user approval.

use crate::ai;
use crate::db;
use crate::docgen;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::tailoring::{self, CoverLetterContent, Finding, ResumeContent};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const KINDS: [&str; 2] = ["resume", "cover_letter"];

fn doc_kind(kind: &str) -> &'static str {
    if kind == "resume" { "generated_resume" } else { "generated_cover_letter" }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    pub kind: String,
    pub job_id: i64,
    pub model: String,
    pub effort: String,
    pub system: String,
    pub user: String,
    pub fact_count: usize,
    pub estimated_usd: f64,
    /// Earlier unsuccessful applications to this employer that could be used as a lesson.
    pub previous_available: Vec<String>,
    pub includes_previous: bool,
    /// How many of the employer's own terms (company research) are included.
    pub company_terms: usize,
}

static RE_CONTACT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)[\w.+-]+@[\w-]+\.[\w.]+|\+?1?[\s.(-]*\d{3}[\s.)-]*\d{3}[\s.-]*\d{4}|linkedin\.com/\S*|https?://\S+").unwrap()
});

/// Removes email addresses, phone numbers and links before text leaves the Mac.
pub fn strip_contact(s: &str) -> String {
    RE_CONTACT.replace_all(s, "[removed]").to_string()
}

/// Replaces the candidate's own name with a placeholder.
pub fn redact_name(s: &str, name: &str) -> String {
    let n = name.trim();
    if n.len() < 3 {
        return s.to_string();
    }
    regex::Regex::new(&format!(r"(?i){}", n.split_whitespace().map(regex::escape).collect::<Vec<_>>().join(r"\s+")))
        .map(|r| r.replace_all(s, "[candidate]").to_string())
        .unwrap_or_else(|_| s.to_string())
}

fn employer_key(e: &str) -> Option<String> {
    const STOP: [&str; 12] = ["the", "inc", "llc", "ltd", "corp", "company", "group", "health", "resources", "university", "of", "and"];
    e.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| w.len() >= 3 && !STOP.contains(w))
        .map(String::from)
}

/// Resume / cover-letter text from earlier applications to the same employer that did not succeed.
pub fn previous_applications(c: &Connection, job: &Job) -> AppResult<Vec<(String, String)>> {
    let Some(key) = employer_key(&job.draft.employer) else { return Ok(vec![]) };
    let mut out: Vec<(String, String)> = vec![];
    for other in db::list_jobs(c)? {
        if other.id == job.id || !["Rejected", "Closed", "Withdrawn"].contains(&other.status.as_str()) || employer_key(&other.draft.employer).as_deref() != Some(key.as_str()) {
            continue;
        }
        let docs = db::list_documents(c, Some(other.id))?;
        for kinds in [["sent_resume", "generated_resume"], ["sent_cover_letter", "generated_cover_letter"]] {
            let mut cands: Vec<&DocumentRec> = docs
                .iter()
                .filter(|d| kinds.contains(&d.kind.as_str()) && d.extracted_text.trim().len() > 200)
                .filter(|d| !d.kind.starts_with("generated_") || d.status == "approved")
                .collect();
            cands.sort_by_key(|d| std::cmp::Reverse(d.created_at.clone()));
            if let Some(d) = cands.first() {
                let what = if kinds[0] == "sent_resume" { "resume" } else { "cover letter" };
                let label = format!("{} at {} ({}), {what}", other.draft.title, other.draft.employer, other.status);
                if !out.iter().any(|(_, t)| t == &d.extracted_text) {
                    out.push((label, d.extracted_text.clone()));
                }
            }
        }
    }
    Ok(out)
}

pub fn prepare(c: &Connection, job_id: i64, kind: &str, learn_from_previous: bool) -> AppResult<Prepared> {
    if !KINDS.contains(&kind) {
        return Err(AppError::invalid("Choose a resume or a cover letter"));
    }
    let s = ai::get_settings(c)?;
    let job = db::get_job(c, job_id)?;
    let p = db::get_profile(c)?;
    if p.experience.is_empty() {
        return Err(AppError::invalid("Add your work history in Career profile first — documents are built only from it"));
    }
    if p.basics.full_name.trim().is_empty() {
        return Err(AppError::invalid("Add your name in Career profile → Contact & summary"));
    }
    let facts = tailoring::facts(&p);
    let system = tailoring::system_prompt(kind, &s.style_notes);
    let company = crate::company::get(c, &job.draft.employer)?;
    let company_block = company.as_ref().map(crate::company::prompt_block).unwrap_or_default();
    let mut user = format!("{company_block}{}", tailoring::user_prompt(&job, &facts, &tailoring::positions(&p)));
    let previous = previous_applications(c, &job)?;
    let includes_previous = learn_from_previous && !previous.is_empty();
    if includes_previous {
        for (label, text) in &previous {
            user.push_str(&format!(
                "\n\n<previous_application note=\"{label}\">\n{}\n</previous_application>",
                redact_name(&strip_contact(&text.chars().take(15_000).collect::<String>()), &p.basics.full_name)
            ));
        }
        user.push_str(
            "\n\nThe candidate applied to this employer before with the documents above and was not selected. \
             Treat them ONLY as a lesson, never as a source: do not cite them, do not reuse their wording, and avoid their likely \
             weaknesses (for example generic phrasing, weak alignment to the posting, the most relevant experience buried, excess length). \
             All facts must still come from candidate_facts. In notes_for_candidate, say briefly what you did differently and why.",
        );
    }
    let expected_out = if kind == "resume" { 7000 } else { 5000 };
    Ok(Prepared {
        kind: kind.into(),
        job_id,
        model: s.model.clone(),
        effort: s.effort.clone(),
        estimated_usd: ai::estimate_usd(&s.model, system.len() + user.len() + 3000, expected_out),
        system,
        user,
        fact_count: facts.len(),
        previous_available: previous.into_iter().map(|(l, _)| l).collect(),
        includes_previous,
        company_terms: company.map(|r| r.terms.iter().filter(|t| t.selected).count()).unwrap_or(0),
    })
}

pub fn request_body(p: &Prepared) -> Value {
    let schema = if p.kind == "resume" { tailoring::resume_schema() } else { tailoring::cover_letter_schema() };
    ai::build_request(&p.model, &p.effort, &p.system, &p.user, &schema)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Generated {
    pub document: DocumentRec,
    pub findings: Vec<Finding>,
    pub cost_usd: f64,
    pub served_by_fallback: bool,
    pub notes_for_candidate: String,
}

fn slug(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    t.split('_').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("_").chars().take(48).collect()
}

/// Validates, checks, renders and stores one version. `content` is ResumeContent / CoverLetterContent JSON.
pub fn save_version(c: &Connection, docs_dir: &Path, job_id: i64, kind: &str, content: &Value) -> AppResult<(DocumentRec, Vec<Finding>, String)> {
    let job = db::get_job(c, job_id)?;
    let p = db::get_profile(c)?;
    let mut facts = tailoring::facts(&p);
    // Statements about the employer may cite its own website (rule 12).
    if let Some(r) = crate::company::get(c, &job.draft.employer)? {
        facts.extend(crate::company::facts(&r));
    }
    let evidence = crate::scoring::profile_facts(&p, &db::master_resume_text(c)?, db::today()).evidence;
    let (blocks, findings, notes, clean) = if kind == "resume" {
        let r: ResumeContent = serde_json::from_value(content.clone()).map_err(|e| AppError::invalid(format!("Resume content is malformed: {e}")))?;
        let r = tailoring::clean_resume(r);
        let f = tailoring::check_resume(&r, &facts, &p.skills, &evidence, &job, &tailoring::positions(&p));
        (tailoring::render_resume(&p, &r), f, r.notes_for_candidate.clone(), serde_json::to_value(&r).map_err(AppError::other)?)
    } else {
        let l: CoverLetterContent = serde_json::from_value(content.clone()).map_err(|e| AppError::invalid(format!("Cover letter content is malformed: {e}")))?;
        let l = tailoring::clean_letter(l);
        let f = tailoring::check_cover_letter(&l, &facts, &evidence, &job);
        let date = chrono::Local::now().format("%B %-d, %Y").to_string();
        (tailoring::render_cover_letter(&p, &job, &l, &date), f, l.notes_for_candidate.clone(), serde_json::to_value(&l).map_err(AppError::other)?)
    };
    let dk = doc_kind(kind);
    let version: i64 = c.query_row("SELECT COALESCE(MAX(version),0)+1 FROM documents WHERE kind=?1 AND job_id=?2", params![dk, job_id], |r| r.get(0))?;
    let label = if kind == "resume" { "Resume" } else { "Cover_Letter" };
    let dir: PathBuf = docs_dir.join("generated").join(slug(&job.draft.employer)).join(slug(&job.draft.title));
    std::fs::create_dir_all(&dir)?;
    let stem = format!("{}_{}_{}_{}_v{}", slug(&p.basics.full_name), slug(&job.draft.employer), slug(&job.draft.title), label, version);
    let docx = dir.join(format!("{stem}.docx"));
    let pdf = dir.join(format!("{stem}.pdf"));
    docgen::write_docx(&blocks, &docx)?;
    docgen::write_pdf(&blocks, &format!("{} - {}", p.basics.full_name, label.replace('_', " ")), &pdf)?;
    let text = tailoring::plain_text(&blocks);
    let sha = crate::documents::sha256_hex(&std::fs::read(&docx)?);
    c.execute(
        "INSERT INTO documents (kind, file_name, stored_path, sha256, extracted_text, job_id, version, created_at, content_json, claim_report, status, pdf_path)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'draft',?11)",
        params![
            dk,
            format!("{stem}.docx"),
            docx.display().to_string(),
            sha,
            text,
            job_id,
            version,
            db::now(),
            clean.to_string(),
            serde_json::to_string(&findings).map_err(AppError::other)?,
            pdf.display().to_string()
        ],
    )?;
    let id = c.last_insert_rowid();
    let blockers = findings.iter().filter(|f| f.severity == "blocker").count();
    db::log(c, "info", "documents", &format!("{label} v{version} for job #{job_id}: {} findings ({blockers} blocking)", findings.len()));
    Ok((db::get_document(c, id)?, findings, notes))
}

/// Turns a raw Messages API response into a stored draft (or a clear error), recording usage either way.
pub fn finish(c: &Connection, docs_dir: &Path, prep: &Prepared, response: &Value) -> AppResult<Generated> {
    let usage = ai::usage_of(response);
    let model = response.get("model").and_then(Value::as_str).unwrap_or(&prep.model).to_string();
    match ai::parse_response(response) {
        Err(e) => {
            let outcome = if matches!(e, ai::AiFailure::Refused(_)) { "refused" } else { "error" };
            ai::record_usage(c, &prep.kind, Some(prep.job_id), &model, &usage, outcome)?;
            Err(AppError::other(e.to_string()))
        }
        Ok(r) => {
            let cost = ai::record_usage(c, &prep.kind, Some(prep.job_id), &model, &r.usage, "ok")?;
            let (document, findings, notes) = save_version(c, docs_dir, prep.job_id, &prep.kind, &r.json)?;
            Ok(Generated { document, findings, cost_usd: cost, served_by_fallback: r.served_by_fallback, notes_for_candidate: notes })
        }
    }
}

/// Saves the user's edits as a new version (re-rendered and re-checked).
pub fn save_edit(c: &Connection, docs_dir: &Path, doc_id: i64, content: &Value) -> AppResult<Generated> {
    let d = db::get_document(c, doc_id)?;
    let kind = match d.kind.as_str() {
        "generated_resume" => "resume",
        "generated_cover_letter" => "cover_letter",
        _ => return Err(AppError::invalid("Only generated documents can be edited here")),
    };
    let job_id = d.job_id.ok_or_else(|| AppError::invalid("Document is not linked to a job"))?;
    let (document, findings, notes) = save_version(c, docs_dir, job_id, kind, content)?;
    Ok(Generated { document, findings, cost_usd: 0.0, served_by_fallback: false, notes_for_candidate: notes })
}

/// Approval requires zero blocking findings, and explicit confirmation of any review items.
pub fn approve(c: &Connection, doc_id: i64, reviewed: bool) -> AppResult<DocumentRec> {
    let d = db::get_document(c, doc_id)?;
    let findings: Vec<Finding> = d.claim_report.clone();
    let blockers = findings.iter().filter(|f| f.severity == "blocker").count();
    if blockers > 0 {
        return Err(AppError::invalid(format!("{blockers} line(s) can't be traced to your verified profile. Edit or remove them, then approve.")));
    }
    if findings.iter().any(|f| f.severity == "review") && !reviewed {
        return Err(AppError::invalid("Confirm you've reviewed the flagged items before approving"));
    }
    let job_id = d.job_id.ok_or_else(|| AppError::invalid("Document is not linked to a job"))?;
    c.execute("UPDATE documents SET status = 'superseded' WHERE kind = ?1 AND job_id = ?2 AND status = 'approved'", params![d.kind, job_id])?;
    c.execute("UPDATE documents SET status = 'approved', approved_at = ?1 WHERE id = ?2", params![db::now(), doc_id])?;
    let job = db::get_job(c, job_id)?;
    if ["Discovered", "Under Review"].contains(&job.status.as_str()) {
        db::change_status(c, &StatusChange { job_id, to_status: "Documents Ready".into(), note: format!("{} approved", d.file_name), confirmation_ref: None, user_confirmed_submission: false })?;
    }
    db::log(c, "info", "documents", &format!("Approved {}", d.file_name));
    db::get_document(c, doc_id)
}

pub fn latest(c: &Connection, job_id: i64, kind: &str) -> AppResult<Option<DocumentRec>> {
    let id: Option<i64> = c
        .query_row("SELECT id FROM documents WHERE kind = ?1 AND job_id = ?2 ORDER BY version DESC LIMIT 1", params![doc_kind(kind), job_id], |r| r.get(0))
        .optional()?;
    id.map(|i| db::get_document(c, i)).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup() -> (Connection, i64, tempfile::TempDir) {
        let c = db::open_in_memory().unwrap();
        let p = crate::tailoring::tests::profile();
        db::save_profile_basics(&c, &p.basics).unwrap();
        for e in &p.experience {
            db::save_experience(&c, e).unwrap();
        }
        for s in &p.skills {
            db::save_skill(&c, s).unwrap();
        }
        for e in &p.education {
            db::save_education(&c, e).unwrap();
        }
        let j = crate::tailoring::tests::job();
        let id = db::insert_job(&c, &j.draft, false, false).unwrap().id.unwrap();
        (c, id, tempfile::tempdir().unwrap())
    }

    fn fake_response(content: Value) -> Value {
        json!({"model": "claude-opus-5-5", "stop_reason": "end_turn",
            "content": [{"type": "text", "text": content.to_string()}],
            "usage": {"input_tokens": 9000, "output_tokens": 4000}})
    }

    #[test]
    fn prepare_requires_profile_and_hides_contact_details() {
        let c = db::open_in_memory().unwrap();
        let j = crate::tailoring::tests::job();
        let id = db::insert_job(&c, &j.draft, false, false).unwrap().id.unwrap();
        assert!(prepare(&c, id, "resume", false).is_err(), "no work history yet");
        let (c, id, _d) = setup();
        let p = prepare(&c, id, "resume", false).unwrap();
        assert!(!p.user.contains("jordan@example.com"));
        assert!(p.estimated_usd > 0.0);
        assert!(prepare(&c, id, "poem", false).is_err());
        let body = request_body(&p);
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    }

    #[test]
    fn full_resume_flow_with_claim_check_and_approval() {
        let (c, id, dir) = setup();
        let prep = prepare(&c, id, "resume", false).unwrap();
        let invented = json!({"headline": "Operations and Smartsheet specialist", "summary": {"text": "Operations specialist with 6+ years in crisis management operations.", "sources": ["S1"]},
            "skills": ["Excel", "Smartsheet"], "notes_for_candidate": "Emphasised Excel.",
            "positions": [{"position_id": "P0", "include": true, "bullets": [
                {"text": "Designed an Excel outreach workflow for a 6,000-record activation supporting 15 agents", "sources": ["E0.0"]},
                {"text": "Reduced reporting time 40% with SQL", "sources": ["E0.1"]}]},
                {"position_id": "P1", "include": false, "bullets": []}]});
        let g = finish(&c, dir.path(), &prep, &fake_response(invented.clone())).unwrap();
        assert!(g.findings.iter().any(|f| f.severity == "blocker"));
        assert!(Path::new(&g.document.stored_path).exists());
        assert!(Path::new(g.document.pdf_path.as_deref().unwrap()).exists());
        assert_eq!(g.document.status, "draft");
        assert_eq!(g.document.version, 1);
        assert!((g.cost_usd - 0.116).abs() < 1e-9, "{}", g.cost_usd);
        assert!(approve(&c, g.document.id, true).is_err(), "blocked lines prevent approval");

        // User removes the invented bullet → new version, clean, approvable
        let mut fixed = invented.clone();
        fixed["positions"][0]["bullets"].as_array_mut().unwrap().pop();
        let g2 = save_edit(&c, dir.path(), g.document.id, &fixed).unwrap();
        assert_eq!(g2.document.version, 2);
        assert!(g2.findings.iter().all(|f| f.severity != "blocker"), "{:?}", g2.findings);
        let a = approve(&c, g2.document.id, true).unwrap();
        assert_eq!(a.status, "approved");
        assert_eq!(db::get_job(&c, id).unwrap().status, "Documents Ready");
        assert!(a.extracted_text.contains("Nov 2023 – Present"));
        assert_eq!(latest(&c, id, "resume").unwrap().unwrap().id, a.id);
        assert_eq!(ai::usage_summary(&c).unwrap().calls, 1);
    }

    #[test]
    fn refusals_are_recorded_and_nothing_is_saved() {
        let (c, id, dir) = setup();
        let prep = prepare(&c, id, "cover_letter", false).unwrap();
        let r = json!({"model": "claude-opus-5-5", "stop_reason": "refusal", "stop_details": {"category": "general_harms"}, "content": [], "usage": {"input_tokens": 100, "output_tokens": 0}});
        assert!(finish(&c, dir.path(), &prep, &r).is_err());
        assert!(latest(&c, id, "cover_letter").unwrap().is_none());
        let outcome: String = c.query_row("SELECT outcome FROM ai_usage", [], |r| r.get(0)).unwrap();
        assert_eq!(outcome, "refused");
    }

    #[test]
    fn learns_from_rejected_application_without_leaking_contact_details() {
        let (c, id, _dir) = setup();
        // An earlier, rejected application to the same employer (different legal-name variant)
        let mut old = crate::tailoring::tests::job().draft;
        old.title = "IT Project Manager".into();
        old.employer = "HSAG Inc".into();
        let old_id = db::insert_job(&c, &old, false, false).unwrap().id.unwrap();
        db::change_status(&c, &StatusChange { job_id: old_id, to_status: "Rejected".into(), note: String::new(), confirmation_ref: None, user_confirmed_submission: false }).unwrap();
        let body = "Jordan Sample | jordan@example.com | 414.555.0100 | linkedin.com/in/jordan\nOperations professional with broad experience in many areas of the business and a strong record.";
        db::insert_document(&c, "sent_resume", "old.pdf", "/x/old.pdf", "h1", &body.repeat(3), Some(old_id)).unwrap();
        db::insert_document(&c, "sent_cover_letter", "a.pdf", "/x/a.pdf", "h2", &"Dear team, I am writing to apply. ".repeat(10), Some(old_id)).unwrap();
        db::insert_document(&c, "sent_cover_letter", "b.pdf", "/x/b.pdf", "h3", &"Dear team, I am writing to apply. ".repeat(10), Some(old_id)).unwrap();

        let without = prepare(&c, id, "resume", false).unwrap();
        assert_eq!(without.previous_available.len(), 2, "resume + one cover letter (identical copies collapse)");
        assert!(!without.includes_previous && !without.user.contains("previous_application"));
        let with = prepare(&c, id, "resume", true).unwrap();
        assert!(with.includes_previous);
        assert!(with.user.contains("<previous_application note=\"IT Project Manager at HSAG Inc (Rejected), resume\">"));
        assert!(with.user.contains("never as a source"));
        assert!(!with.user.contains("Jordan Sample") && with.user.contains("[candidate]"));
        assert!(!with.user.contains("jordan@example.com") && !with.user.contains("414.555.0100") && !with.user.contains("linkedin.com/in/jordan"));
        assert_eq!(strip_contact("call +1 (414) 555-0100 or a.b@c.org"), "call [removed] or [removed]");
    }

    #[test]
    fn cover_letter_flow() {
        let (c, id, dir) = setup();
        let prep = prepare(&c, id, "cover_letter", false).unwrap();
        let letter = json!({"greeting": "Dear Hiring Team,", "closing": "Sincerely,", "notes_for_candidate": "",
            "paragraphs": [{"text": "HSAG's Analytics Coordinator III role fits the dashboard work I do every day.", "sources": ["E0.1"]},
                           {"text": "I built Smartsheet dashboards and testing trackers.", "sources": ["E0.1"]}]});
        let g = finish(&c, dir.path(), &prep, &fake_response(letter)).unwrap();
        assert!(g.findings.iter().all(|f| f.severity != "blocker"), "{:?}", g.findings);
        assert!(g.document.extracted_text.contains("Re: Analytics Coordinator III"));
        assert!(g.document.file_name.contains("Cover_Letter_v1"));
    }
}
