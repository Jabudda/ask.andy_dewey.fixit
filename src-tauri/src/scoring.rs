//! Explainable 0–100 compatibility scoring.
//!
//! Rules of evidence:
//! * Credit is given only for skills, tools, titles, certifications and education found in the
//!   applicant's **verified** profile (skills, positions, achievements, certifications,
//!   accomplishments, education) or their own master resume text.
//! * Requirements the app cannot judge are listed as "unverified" — never counted as met.
//! * Inferences (seniority from titles, total years) are labelled as assumptions.

use crate::documents::suggest_skills;
use crate::models::*;
use chrono::{Datelike, NaiveDate};
use regex::Regex;
use std::sync::LazyLock;

/// Everything the scorer may treat as true about the applicant.
#[derive(Debug, Clone, Default)]
pub struct ProfileFacts {
    /// Lower-cased concatenation of all verified sources.
    pub evidence: String,
    pub titles: Vec<String>,
    pub years: f64,
    /// 0 none · 1 associate · 2 bachelor · 3 master · 4 doctorate
    pub degree_level: u8,
    pub level: f64,
    pub skill_count: usize,
    pub position_count: usize,
    pub has_resume: bool,
    pub positions: Vec<Experience>,
    pub today: NaiveDate,
}

fn parse_ym(s: &str) -> Option<(i32, u32)> {
    let s = s.trim();
    let y: i32 = s.get(..4)?.parse().ok()?;
    let m: u32 = s.get(5..7).and_then(|m| m.parse().ok()).unwrap_or(1);
    (1950..2100).contains(&y).then_some((y, m.clamp(1, 12)))
}

/// Total years covered by positions, merging overlaps.
pub fn experience_years(exp: &[Experience], today: NaiveDate) -> f64 {
    let mut spans: Vec<(i32, i32)> = exp
        .iter()
        .filter_map(|e| {
            let (sy, sm) = parse_ym(&e.start_date)?;
            let end = if e.is_current || e.end_date.trim().is_empty() {
                (today.year(), today.month())
            } else {
                parse_ym(&e.end_date).map(|(y, m)| (y, if e.end_date.trim().len() <= 4 { 12 } else { m }))?
            };
            let a = sy * 12 + sm as i32 - 1;
            let b = end.0 * 12 + end.1 as i32;
            (b > a).then_some((a, b))
        })
        .collect();
    spans.sort();
    let mut total = 0;
    let mut cur: Option<(i32, i32)> = None;
    for (a, b) in spans {
        cur = match cur {
            Some((ca, cb)) if a <= cb => Some((ca, cb.max(b))),
            Some((ca, cb)) => {
                total += cb - ca;
                Some((a, b))
            }
            None => Some((a, b)),
        };
    }
    if let Some((a, b)) = cur {
        total += b - a;
    }
    total as f64 / 12.0
}

fn degree_level(text: &str) -> u8 {
    let t = text.to_lowercase();
    if Regex::new(r"ph\.?d|doctor").unwrap().is_match(&t) {
        4
    } else if Regex::new(r"master|\bmba\b|\bm\.?s\.?\b|\bm\.?a\.?\b").unwrap().is_match(&t) {
        3
    } else if Regex::new(r"bachelor|\bb\.?a\.?\b|\bb\.?s\.?\b|undergraduate").unwrap().is_match(&t) {
        2
    } else if t.contains("associate") {
        1
    } else {
        0
    }
}

fn title_level(title: &str) -> f64 {
    let t = format!(" {} ", title.to_lowercase().replace([',', '-', '(', ')', '/'], " "));
    let has = |w: &str| t.contains(&format!(" {w} "));
    if has("vp") || has("vice") || has("chief") {
        5.0
    } else if has("director") || has("head") {
        4.5
    } else if has("manager") || has("principal") {
        3.5
    } else if has("senior") || has("sr") || has("lead") || has("iii") || has("staff") {
        3.0
    } else if has("ii") {
        2.5
    } else if has("intern") || has("junior") || has("jr") || has("entry") || has("trainee") {
        1.0
    } else {
        2.0
    }
}

pub fn profile_facts(p: &ProfileBundle, master_resume_text: &str, today: NaiveDate) -> ProfileFacts {
    let mut ev = vec![p.basics.headline.clone(), p.basics.summary.clone(), master_resume_text.to_string()];
    for s in &p.skills {
        ev.push(s.name.clone());
    }
    for e in &p.experience {
        ev.push(format!("{} {} {}", e.title, e.employer, e.achievements));
    }
    for c in &p.certifications {
        ev.push(format!("{} {}", c.name, c.issuer));
    }
    for a in &p.accomplishments {
        ev.push(format!("{} {}", a.title, a.description));
    }
    for e in &p.education {
        ev.push(format!("{} {} {} {}", e.degree, e.field, e.minor, e.institution));
    }
    let titles: Vec<String> = p.experience.iter().map(|e| e.title.clone()).collect();
    // Recent roles (last 10 years) define the working level; older management roles are history.
    let recent_level = p
        .experience
        .iter()
        .filter(|e| e.is_current || parse_ym(&e.end_date).map(|(y, _)| y >= today.year() - 10).unwrap_or(false))
        .map(|e| title_level(&e.title))
        .fold(0.0, f64::max);
    ProfileFacts {
        evidence: ev.join("\n").to_lowercase(),
        years: experience_years(&p.experience, today),
        degree_level: p.education.iter().map(|e| degree_level(&format!("{} {}", e.degree, e.notes))).max().unwrap_or(0),
        level: if recent_level > 0.0 { recent_level } else { 2.0 },
        titles,
        skill_count: p.skills.len(),
        position_count: p.experience.len(),
        has_resume: !master_resume_text.trim().is_empty(),
        positions: p.experience.clone(),
        today,
    }
}

// ───────────────────────────── term matching ─────────────────────────────

const ALIASES: &[(&str, &[&str])] = &[
    ("user acceptance testing", &["uat"]),
    ("uat", &["user acceptance testing"]),
    ("data cleanup", &["data cleansing", "data cleaning", "data quality"]),
    ("data cleansing", &["data cleanup", "data cleaning"]),
    ("data quality", &["data cleanup", "data integrity", "data validation"]),
    ("microsoft 365", &["office 365", "m365", "microsoft office"]),
    ("nice cxone", &["cxone", "incontact", "nice incontact"]),
    ("contact center", &["call center", "virtual call center", "vcc"]),
    ("call center", &["contact center", "virtual call center"]),
    ("stakeholder management", &["stakeholder", "stakeholders"]),
    ("stakeholder communication", &["stakeholder", "stakeholders"]),
    ("documentation", &["user guide", "user guidance", "sop", "standard operating procedures"]),
    ("dashboards", &["dashboard"]),
    ("reporting", &["reports", "report"]),
    ("project coordination", &["project coordinator", "coordinated"]),
    ("workflow automation", &["automated", "automation", "power automate", "office scripts"]),
    ("troubleshooting", &["troubleshoot", "troubleshot"]),
    ("microsoft office", &["excel", "microsoft 365", "office 365", "word", "outlook", "powerpoint", "sharepoint", "microsoft teams"]),
];

fn term_re(term: &str) -> Regex {
    crate::documents::term_regex(term)
}

pub fn has_evidence(evidence: &str, term: &str) -> bool {
    let t = term.to_lowercase();
    if term_re(&t).is_match(evidence) {
        return true;
    }
    ALIASES
        .iter()
        .filter(|(k, _)| *k == t)
        .flat_map(|(_, alts)| alts.iter())
        .any(|a| term_re(a).is_match(evidence))
}

/// Vocabulary + preference keywords that appear in `text`, de-duplicated.
fn terms_in(text: &str, extra: &[String]) -> Vec<String> {
    let mut v = suggest_skills(text);
    for k in extra {
        let k = k.trim();
        if k.len() >= 3 && term_re(k).is_match(text) && !v.iter().any(|x| x.eq_ignore_ascii_case(k)) {
            v.push(k.to_string());
        }
    }
    // Drop terms contained in a longer matched term ("Smartsheet" inside "Smartsheet Control Center").
    let lower: Vec<String> = v.iter().map(|x| x.to_lowercase()).collect();
    v.into_iter()
        .enumerate()
        .filter(|(i, t)| {
            let tl = t.to_lowercase();
            !lower.iter().enumerate().any(|(j, o)| j != *i && o.len() > tl.len() && o.contains(&tl) && term_re(&tl).is_match(o))
        })
        .map(|(_, t)| t)
        .collect()
}

// ───────────────────────────── requirement sections ─────────────────────────────

static RE_REQ_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\W*(minimum qualifications|basic qualifications|required qualifications|requirements|qualifications|required skills|what you(?:'|’)?ll need|what you need|what you bring|what we(?:'|’)re looking for|who you are|must[- ]haves?|you have|you may be a fit|about you|skills (?:and|&) experience|experience|knowledge, skills)").unwrap()
});
static RE_PREF_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\W*(preferred|nice[- ]to[- ]haves?|bonus|it(?:'|’)s a plus|plus(?:es)?|desired|ideal(?:ly)?|extra credit|additional qualifications)").unwrap()
});
static RE_OTHER_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\W*(responsibilities|what you(?:'|’)?ll do|what you will do|the role|about (?:the|us|our)|benefits|perks|compensation|salary|pay|why join|our team|the team|who we are|location|equal opportunity|eeo|how to apply|day to day|duties|in this role)").unwrap()
});
static RE_YEARS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(\d{1,2})\s*\+?\s*(?:-\s*\d{1,2}\s*)?(?:years?|yrs?)").unwrap());
static RE_PEOPLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:manag\w*|supervis\w*|lead\w*|train\w*|coach\w*|mentor\w*)\b[^.;]{0,25}\b(?:staff|teams?|people|employees|agents|direct reports|others)\b|\bpeople management\b|\bdirect reports\b").unwrap()
});
static RE_PEOPLE_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:manager|supervisor|lead|director|head|owner)\b").unwrap());
static RE_PEOPLE_WORK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:managed a team|team of \d+|supervised|coaching|coached|led training|training initiatives|trained)\b").unwrap());
static RE_DEGREE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)bachelor|\bb\.?a\.?\b|\bb\.?s\.?\b|master'?s|\bmba\b|ph\.?d|associate'?s degree|\bdegree\b").unwrap());
static RE_EQUIV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)or equivalent|equivalent (?:work )?experience|or related experience|in lieu of").unwrap());
static RE_CLEARANCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)security clearance|secret clearance|top secret|ts/sci|public trust").unwrap());
pub const CERT_TERMS: &[&str] = &[
    "PMP", "CAPM", "ITIL", "CompTIA", "A+", "Network+", "Security+", "Six Sigma", "Green Belt", "Black Belt",
    "Scrum Master", "CSM", "PMI-ACP", "Smartsheet certification", "Smartsheet Certified", "SHRM", "CPA", "CCNA",
];

/// Short line that introduces a section ("Requirements:", "What you'll need"). Bullets never are.
fn is_heading(line: &str) -> bool {
    let words = line.split_whitespace().count();
    let t = line.trim_start();
    !t.starts_with(['•', '-', '*', '·', '–']) && line.len() <= 70 && (1..=6).contains(&words) && (line.trim_end().ends_with(':') || words <= 4)
}

/// (required lines, preferred lines) from a posting.
pub fn requirement_lines(description: &str) -> (Vec<String>, Vec<String>) {
    #[derive(PartialEq)]
    enum Sec {
        None,
        Req,
        Pref,
    }
    let mut sec = Sec::None;
    let (mut req, mut pref) = (vec![], vec![]);
    for raw in description.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if is_heading(line) {
            if RE_PREF_HEAD.is_match(line) {
                sec = Sec::Pref;
                continue;
            }
            if RE_REQ_HEAD.is_match(line) {
                sec = Sec::Req;
                continue;
            }
            if RE_OTHER_HEAD.is_match(line) || line.ends_with(':') {
                sec = Sec::None;
                continue;
            }
        }
        let clean = line.trim_start_matches(['•', '-', '*', '·', '–', '●', '▪', '◦']).trim().to_string();
        if clean.split_whitespace().count() < 2 {
            continue;
        }
        let lower = clean.to_lowercase();
        let inline_pref = lower.contains("preferred") || lower.contains("nice to have") || lower.starts_with("bonus");
        match sec {
            Sec::Req if inline_pref => pref.push(clean),
            Sec::Req => req.push(clean),
            Sec::Pref => pref.push(clean),
            Sec::None => {}
        }
    }
    if req.is_empty() {
        // No recognisable section: fall back to sentences that read like requirements.
        let re = Regex::new(r"(?i)\b(required|must have|must be|experience (?:with|in)|proficien|years of|knowledge of|familiarity with|ability to)\b").unwrap();
        for raw in description.split(['\n', '.']) {
            let l = raw.trim().trim_start_matches(['•', '-', '*']).trim();
            if l.split_whitespace().count() >= 4 && re.is_match(l) && req.len() < 12 {
                if l.to_lowercase().contains("preferred") {
                    pref.push(l.to_string());
                } else {
                    req.push(l.to_string());
                }
            }
        }
    }
    req.truncate(25);
    pref.truncate(15);
    (req, pref)
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Met(String),
    Partial(String),
    Gap(String, bool),
    Unverified(String),
}

fn short(s: &str) -> String {
    let s: String = s.chars().take(140).collect();
    if s.len() >= 140 { format!("{s}…") } else { s }
}

fn judge(line: &str, f: &ProfileFacts, required: bool, extra: &[String]) -> Verdict {
    let l = short(line);
    if RE_CLEARANCE.is_match(line) {
        return Verdict::Gap(format!("{l} — no clearance in your profile"), required);
    }
    if let Some(c) = RE_YEARS.captures(line) {
        let need: f64 = c[1].parse().unwrap_or(0.0);
        if need > 0.0 && need < 40.0 {
            // Count only the roles that involve what the requirement names, when it names something.
            let topics = terms_in(line, extra);
            let people = RE_PEOPLE.is_match(line);
            if !topics.is_empty() || people {
                let rel: Vec<Experience> = f
                    .positions
                    .iter()
                    .filter(|e| {
                        let t = format!("{} {}", e.title, e.achievements).to_lowercase();
                        topics.iter().any(|x| has_evidence(&t, x)) || (people && (RE_PEOPLE_TITLE.is_match(&e.title) || RE_PEOPLE_WORK.is_match(&e.achievements)))
                    })
                    .cloned()
                    .collect();
                let mut what = topics.join(", ");
                if people {
                    what = if what.is_empty() { "managing, leading or training people".into() } else { format!("{what} or leading people") };
                }
                if rel.is_empty() {
                    return Verdict::Gap(format!("{l} — none of your listed roles mention {what}"), false);
                }
                let ry = experience_years(&rel, f.today);
                let names = rel.iter().map(|e| e.title.as_str()).take(3).collect::<Vec<_>>().join(", ");
                return if ry + 0.01 >= need {
                    Verdict::Met(format!("{l} — about {ry:.1} years in roles that mention {what} ({names})"))
                } else if ry + 1.0 >= need {
                    Verdict::Partial(format!("{l} — about {ry:.1} years in roles that mention {what} ({names})"))
                } else {
                    Verdict::Gap(format!("{l} — about {ry:.1} years in roles that mention {what} ({names})"), required && need - ry > 2.0)
                };
            }
            return if f.years + 0.01 >= need {
                Verdict::Met(format!("{l} — you have {:.0}+ years across your listed positions", f.years.floor()))
            } else if f.years + 2.0 >= need {
                Verdict::Partial(format!("{l} — your listed positions cover about {:.1} years", f.years))
            } else {
                Verdict::Gap(format!("{l} — your listed positions cover about {:.1} years", f.years), required && need - f.years > 2.0)
            };
        }
    }
    if RE_DEGREE.is_match(line) && (line.to_lowercase().contains("degree") || degree_level(line) > 0) {
        let need = degree_level(line).max(1);
        let equiv = RE_EQUIV.is_match(line);
        return if f.degree_level >= need {
            Verdict::Met(format!("{l} — matches your education record"))
        } else if equiv && f.years >= 4.0 {
            Verdict::Met(format!("{l} — accepts equivalent experience ({:.0}+ years)", f.years.floor()))
        } else if f.degree_level == 0 {
            Verdict::Unverified(format!("{l} — add your education to the profile so this can be checked"))
        } else {
            Verdict::Gap(format!("{l} — above the degree in your profile"), required && !equiv)
        };
    }
    let certs: Vec<&str> = CERT_TERMS.iter().copied().filter(|c| term_re(c).is_match(line)).collect();
    if !certs.is_empty() {
        let have: Vec<&str> = certs.iter().copied().filter(|c| has_evidence(&f.evidence, c)).collect();
        let either = line.to_lowercase().contains(" or ");
        return if have.len() == certs.len() || (either && !have.is_empty()) {
            Verdict::Met(format!("{l} — certification in your profile"))
        } else {
            Verdict::Gap(format!("{l} — {} not in your profile", certs.join(", ")), required && !line.to_lowercase().contains("prefer"))
        };
    }
    let terms = terms_in(line, extra);
    if terms.is_empty() {
        return Verdict::Unverified(l);
    }
    let (have, miss): (Vec<String>, Vec<String>) = terms.into_iter().partition(|t| has_evidence(&f.evidence, t));
    if miss.is_empty() {
        Verdict::Met(format!("{l} — {}", have.join(", ")))
    } else if !have.is_empty() {
        Verdict::Partial(format!("{l} — have {}; missing {}", have.join(", "), miss.join(", ")))
    } else {
        Verdict::Gap(format!("{l} — {} not in your profile", miss.join(", ")), false)
    }
}

// ───────────────────────────── scoring ─────────────────────────────

const TITLE_STOP: &[&str] = &[
    "senior", "sr", "junior", "jr", "lead", "principal", "staff", "ii", "iii", "iv", "i", "remote", "the", "and", "of",
    "for", "contract", "temporary", "part", "time", "full", "us", "usa", "hybrid", "a", "to", "in", "at", "with",
];

fn title_tokens(t: &str) -> Vec<String> {
    t.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2 && !TITLE_STOP.contains(w))
        .map(String::from)
        .collect()
}

pub fn title_similarity(job: &str, other: &str) -> f64 {
    let a = title_tokens(job);
    let b = title_tokens(other);
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let common = a.iter().filter(|t| b.contains(t)).count() as f64;
    // Weighted toward the job title: what fraction of its words does the candidate title share?
    (common / a.len() as f64) * 0.7 + (common / b.len() as f64) * 0.3
}

pub fn score(d: &JobDraft, salary_flag: &str, excluded: &[String], f: &ProfileFacts, p: &Preferences) -> ScoreBreakdown {
    let w = &p.scoring_weights;
    let text = format!("{}\n{}", d.title, d.description);
    let mut out = ScoreBreakdown::default();
    let mut crit: Vec<CriterionScore> = vec![];
    let mut mandatory_gap = false;

    if f.skill_count == 0 && !f.has_resume {
        out.assumptions.push("Your profile has no verified skills or master resume yet, so matches are limited. Scores rise as you complete Career profile.".into());
    }

    // Title
    let (best_t, best_src) = p
        .target_titles
        .iter()
        .map(|t| (title_similarity(&d.title, t), format!("target title “{t}”")))
        .chain(f.titles.iter().map(|t| (title_similarity(&d.title, t), format!("your past title “{t}”"))))
        .fold((0.0, String::new()), |a, b| if b.0 > a.0 { b } else { a });
    crit.push(CriterionScore {
        key: "title".into(),
        label: "Title & field alignment".into(),
        weight: w.title,
        score: best_t.min(1.0),
        detail: if best_t > 0.0 { format!("Closest match: {best_src} ({:.0}%)", best_t * 100.0) } else { "No overlap with your target or past titles".into() },
    });

    // Skills & tools
    let terms = terms_in(&text, &p.keywords);
    let (have, miss): (Vec<String>, Vec<String>) = terms.iter().cloned().partition(|t| has_evidence(&f.evidence, t));
    // Small samples are pulled toward neutral: one matched tool is weak evidence of a strong fit.
    let skill_score = (have.len() as f64 + 1.0) / (terms.len() as f64 + 2.0);
    crit.push(CriterionScore {
        key: "skills".into(),
        label: "Skills & tools".into(),
        weight: w.skills,
        score: skill_score,
        detail: if terms.is_empty() {
            "No recognisable tools or skills in the posting (counted as neutral)".into()
        } else {
            format!(
                "{} of {} found in your profile{}{}",
                have.len(),
                terms.len(),
                if have.is_empty() { String::new() } else { format!(": {}", have.join(", ")) },
                if miss.is_empty() { String::new() } else { format!(". Not in profile: {}", miss.join(", ")) }
            )
        },
    });
    if !have.is_empty() {
        out.satisfied.push(format!("Skills/tools you've verified: {}", have.join(", ")));
    }

    // Required & preferred qualifications
    let (req, pref) = requirement_lines(&d.description);
    let mut section = |lines: &[String], required: bool, out: &mut ScoreBreakdown| -> (f64, usize, usize) {
        let (mut sum, mut judged) = (0.0, 0usize);
        for line in lines {
            match judge(line, f, required, &p.keywords) {
                Verdict::Met(s) => {
                    sum += 1.0;
                    judged += 1;
                    out.satisfied.push(s);
                }
                Verdict::Partial(s) => {
                    sum += 0.5;
                    judged += 1;
                    out.partial.push(s);
                }
                Verdict::Gap(s, mandatory) => {
                    judged += 1;
                    if mandatory {
                        mandatory_gap = true;
                    }
                    out.gaps.push(if required { s } else { format!("(preferred) {s}") });
                }
                Verdict::Unverified(s) => out.unverified.push(if required { s } else { format!("(preferred) {s}") }),
            }
        }
        // Requirements that cannot be checked count slightly below neutral (unknown is not "met").
        let unknown = lines.len() - judged;
        let total = judged + unknown;
        (if total == 0 { 0.5 } else { (sum + 0.4 * unknown as f64) / total as f64 }, judged, lines.len())
    };
    let (rs, rj, rn) = section(&req, true, &mut out);
    crit.push(CriterionScore {
        key: "required".into(),
        label: "Required qualifications".into(),
        weight: w.required,
        score: rs,
        detail: if rn == 0 { "No requirements section found (neutral)".into() } else { format!("{rj} of {rn} requirements could be checked against your profile") },
    });
    let (ps, pj, pn) = section(&pref, false, &mut out);
    if pn > 0 {
        crit.push(CriterionScore {
            key: "preferred".into(),
            label: "Preferred qualifications".into(),
            weight: w.preferred,
            score: ps,
            detail: format!("{pj} of {pn} preferred items could be checked"),
        });
    }

    // Compensation
    let (cs, cd) = match salary_flag {
        "meets" => (1.0, "Listed pay meets your minimum"),
        "partial" => (0.6, "Pay range straddles your minimum"),
        "below" => (0.0, "Listed pay is below your minimum"),
        _ => (0.5, "Pay not listed (counted as neutral, flagged for review)"),
    };
    crit.push(CriterionScore { key: "compensation".into(), label: "Compensation".into(), weight: w.compensation, score: cs, detail: cd.into() });

    // Remote eligibility
    let (rms, rmd) = match (d.remote_type.as_str(), d.wisconsin_eligibility.as_str()) {
        (_, "excluded") => (0.0, format!("Does not hire in {}", p.home_state)),
        ("hybrid" | "onsite", _) => (0.0, "Not fully remote".to_string()),
        ("remote", "eligible") => (1.0, format!("Fully remote and open to {}", p.home_state)),
        ("remote", _) => (0.7, format!("Remote, but hiring in {} is not confirmed", p.home_state)),
        _ => (0.4, "Work arrangement not stated".to_string()),
    };
    crit.push(CriterionScore { key: "remote".into(), label: "Remote eligibility".into(), weight: w.remote, score: rms, detail: rmd });

    // Seniority (inferred)
    let jl = title_level(&d.title);
    let gap = (jl - f.level - 0.5).max(0.0);
    let (sen_score, sen_detail) = if d.internal_candidate {
        (1.0, format!("Internal candidate: you already work at {} and know the role's real scope (title level not used)", d.employer))
    } else {
        ((1.0 - gap / 2.5).clamp(0.0, 1.0), format!("Posting reads as level {jl:.1}; your recent roles read as level {:.1} (inferred from titles)", f.level))
    };
    crit.push(CriterionScore { key: "seniority".into(), label: "Seniority & scope".into(), weight: w.seniority, score: sen_score, detail: sen_detail });
    if !d.internal_candidate && jl - f.level >= 1.5 {
        out.partial.push(format!("Seniority: “{}” is a step above your recent titles — emphasise scope and leadership you have actually held", d.title));
    }
    out.assumptions.push("Seniority is inferred from job titles, not responsibilities.".into());
    if f.years > 0.0 {
        out.assumptions.push(format!("Years of experience ({:.1}) count every position in your profile, related or not.", f.years));
    }

    // Certifications mentioned anywhere
    let certs: Vec<&str> = CERT_TERMS.iter().copied().filter(|c| term_re(c).is_match(&text)).collect();
    if !certs.is_empty() {
        let held = certs.iter().filter(|c| has_evidence(&f.evidence, c)).count();
        crit.push(CriterionScore {
            key: "certifications".into(),
            label: "Certifications".into(),
            weight: w.certifications,
            score: held as f64 / certs.len() as f64,
            detail: format!("Posting mentions {}; {} in your profile", certs.join(", "), held),
        });
    }

    let wsum: f64 = crit.iter().map(|c| c.weight.max(0.0)).sum();
    let raw = if wsum > 0.0 { crit.iter().map(|c| c.weight.max(0.0) * c.score).sum::<f64>() / wsum * 100.0 } else { 0.0 };
    let mut total = raw.round() as i64;
    if !excluded.is_empty() {
        total = total.min(39);
        for e in excluded {
            out.gaps.insert(0, format!("Mandatory requirement: {e}"));
        }
        out.assumptions.push("Capped at 39 because the posting fails one of your mandatory requirements.".into());
    } else if mandatory_gap {
        total = total.min(59);
        out.assumptions.push("Capped at 59 because a required qualification is clearly missing.".into());
    }
    let thin = terms.len() < 3 && rj < 2;
    if thin && total > 74 {
        total = 74;
        out.assumptions.push("Capped at 74: the posting has too little detail (few tools, no requirement list) to confirm a strong match — read it before applying.".into());
    }
    out.recommendation = if !excluded.is_empty() {
        "skip"
    } else if total >= 75 && !mandatory_gap && !thin {
        "apply"
    } else if total >= 55 {
        "consider"
    } else {
        "skip"
    }
    .into();
    out.total = total;
    out.criteria = crit;
    out
}

/// Plain-language hiring-manager assessment built only from the breakdown.
pub fn assessment(d: &JobDraft, b: &ScoreBreakdown) -> String {
    let rec = match b.recommendation.as_str() {
        "apply" => "Recommended — apply.",
        "consider" => "Worth considering — apply if you can speak to the gaps below.",
        _ => "Not recommended as-is.",
    };
    let strongest: Vec<&CriterionScore> = {
        let mut v: Vec<&CriterionScore> = b.criteria.iter().filter(|c| c.score >= 0.7 && c.weight > 0.0).collect();
        v.sort_by(|a, z| (z.weight * z.score).partial_cmp(&(a.weight * a.score)).unwrap());
        v.into_iter().take(3).collect()
    };
    let mut s = format!("{rec} Compatibility {}/100 for {} at {}.\n", b.total, d.title, d.employer);
    if !strongest.is_empty() {
        s.push_str("\nWhy you fit: ");
        s.push_str(&strongest.iter().map(|c| format!("{} — {}", c.label.to_lowercase(), c.detail)).collect::<Vec<_>>().join("; "));
        s.push_str(".\n");
    }
    let list = |title: &str, v: &[String], s: &mut String| {
        if !v.is_empty() {
            s.push_str(&format!("\n{title}:\n"));
            for x in v.iter().take(8) {
                s.push_str(&format!("• {x}\n"));
            }
            if v.len() > 8 {
                s.push_str(&format!("• …and {} more\n", v.len() - 8));
            }
        }
    };
    list("Requirements met", &b.satisfied, &mut s);
    list("Partially met", &b.partial, &mut s);
    list("Gaps", &b.gaps, &mut s);
    list("Could not be checked — your judgment", &b.unverified, &mut s);
    s.push_str("\nBasis: your verified profile and master resume only. ");
    s.push_str(&b.assumptions.join(" "));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::SKILL_VOCABULARY;

    fn facts() -> ProfileFacts {
        let p = ProfileBundle {
            basics: ProfileBasics { headline: "Business Operations Specialist".into(), ..Default::default() },
            experience: vec![
                Experience {
                    employer: "Contoso".into(),
                    title: "Business Operations Specialist".into(),
                    start_date: "2020-08".into(),
                    is_current: true,
                    achievements: "Built Smartsheet dashboards\nAutomated Excel with Office Scripts\nLed UAT for SharePoint rollout".into(),
                    ..Default::default()
                },
                Experience { employer: "Fabrikam Hotels".into(), title: "Guest Service".into(), start_date: "2002".into(), end_date: "2020".into(), ..Default::default() },
            ],
            education: vec![Education { institution: "State University".into(), degree: "Bachelor's degree".into(), field: "Public Relations".into(), ..Default::default() }],
            skills: vec![Skill { name: "Microsoft Teams".into(), ..Default::default() }],
            certifications: vec![Certification { name: "Smartsheet Control Center for Program Leads".into(), ..Default::default() }],
            accomplishments: vec![],
            documents: vec![],
        };
        profile_facts(&p, "", NaiveDate::from_ymd_opt(2026, 11, 4).unwrap())
    }

    fn job(title: &str, desc: &str) -> JobDraft {
        JobDraft {
            title: title.into(),
            employer: "Acme".into(),
            description: desc.into(),
            remote_type: "remote".into(),
            wisconsin_eligibility: "eligible".into(),
            ..Default::default()
        }
    }

    #[test]
    fn internal_candidate_skips_title_seniority_penalty() {
        let f = facts();
        let mut d = job("Manager of Intake", "Qualifications\n• Minimum of 2 years of experience in managing or training staff");
        let outside = score(&d, "meets", &[], &f, &Preferences::default());
        assert!(outside.partial.iter().any(|x| x.starts_with("Seniority")), "{:?}", outside.partial);
        d.internal_candidate = true;
        let inside = score(&d, "meets", &[], &f, &Preferences::default());
        assert!(!inside.partial.iter().any(|x| x.starts_with("Seniority")), "{:?}", inside.partial);
        let sen = inside.criteria.iter().find(|c| c.key == "seniority").unwrap();
        assert_eq!(sen.score, 1.0);
        assert!(sen.detail.starts_with("Internal candidate"));
        assert!(inside.total >= outside.total);
    }

    #[test]
    fn years_merge_overlaps() {
        let f = facts();
        // 2002-01..2020-12 and 2020-08..2026-11 merge to 2002-01..2026-11 ≈ 24.9 years
        assert!((f.years - 24.9).abs() < 0.2, "{}", f.years);
        assert_eq!(f.degree_level, 2);
        assert_eq!(f.level, 2.0);
    }

    #[test]
    fn evidence_and_aliases() {
        let f = facts();
        assert!(has_evidence(&f.evidence, "Smartsheet"));
        assert!(has_evidence(&f.evidence, "User Acceptance Testing"), "UAT alias");
        assert!(!has_evidence(&f.evidence, "SQL"));
        assert!(!has_evidence("used sqlite", "SQL"));
    }

    #[test]
    fn sections_are_split() {
        let d = "About us\nWe build things.\nRequirements:\n• 3+ years of experience in operations\n• Advanced Excel and Smartsheet\n• Experience with SQL and Tableau\nNice to have\n• PMP certification\nBenefits\n• Health insurance for you and family";
        let (r, p) = requirement_lines(d);
        assert_eq!(r.len(), 3, "{r:?}");
        assert_eq!(p, vec!["PMP certification"]);
    }

    #[test]
    fn strong_match_scores_high_and_explains() {
        let f = facts();
        let d = job(
            "Smartsheet Administrator",
            "Responsibilities\n• Own the Smartsheet environment\nQualifications\n• 3+ years of operations experience\n• Advanced Excel including Office Scripts\n• Experience running UAT and writing documentation\n• Bachelor's degree or equivalent experience",
        );
        let b = score(&d, "meets", &[], &f, &Preferences::default());
        assert!(b.total >= 80, "{} {:?}", b.total, b.criteria);
        assert_eq!(b.recommendation, "apply");
        assert!(b.gaps.is_empty(), "{:?}", b.gaps);
        let a = assessment(&d, &b);
        assert!(a.starts_with("Recommended"));
        assert!(a.contains("Basis: your verified profile"));
    }

    #[test]
    fn missing_skills_and_mandatory_gaps_lower_score() {
        let f = facts();
        let d = job(
            "Senior Data Engineer",
            "Requirements\n• 8+ years of experience building data pipelines\n• Expert SQL, Python and Tableau\n• Master's degree in Computer Science required\n• Active security clearance",
        );
        let b = score(&d, "missing", &[], &f, &Preferences::default());
        assert!(b.total <= 59, "{}", b.total);
        assert!(b.gaps.iter().any(|g| g.contains("SQL") || g.contains("Python")));
        assert!(b.gaps.iter().any(|g| g.contains("clearance")));
        assert!(b.gaps.iter().any(|g| g.contains("Master")));
        assert_ne!(b.recommendation, "apply");
    }

    #[test]
    fn excluded_jobs_are_capped() {
        let f = facts();
        let d = job("Smartsheet Administrator", "Requirements\n• Smartsheet and Excel");
        let b = score(&d, "meets", &["Hybrid role".to_string()], &f, &Preferences::default());
        assert!(b.total <= 39);
        assert_eq!(b.recommendation, "skip");
        assert!(b.gaps[0].starts_with("Mandatory requirement"));
    }

    #[test]
    fn unknown_requirements_are_not_counted_as_met() {
        let f = facts();
        let d = job("Operations Coordinator", "Requirements\n• Thrives in ambiguity and moves fast\n• Excellent written communication skills overall");
        let b = score(&d, "meets", &[], &f, &Preferences::default());
        assert_eq!(b.unverified.len(), 2);
        assert!(b.satisfied.iter().all(|s| !s.contains("ambiguity")));
    }

    #[test]
    fn weights_change_the_score() {
        let f = facts();
        let d = job("Data Analyst", "Requirements\n• SQL and Tableau dashboards");
        let mut p = Preferences::default();
        let base = score(&d, "meets", &[], &f, &p).total;
        p.scoring_weights.skills = 0.0;
        p.scoring_weights.required = 0.0;
        let without_skills = score(&d, "meets", &[], &f, &p).total;
        assert!(without_skills > base, "{without_skills} vs {base}");
    }

    #[test]
    fn thin_postings_are_not_recommended_outright() {
        let f = facts();
        let d = job("Operations Coordinator", "Join our team! Great culture.");
        let b = score(&d, "meets", &[], &f, &Preferences::default());
        assert!(b.total <= 74, "{}", b.total);
        assert_eq!(b.recommendation, "consider");
        assert!(b.assumptions.iter().any(|a| a.contains("too little detail")));
    }

    #[test]
    fn little_evidence_is_not_a_perfect_score() {
        let f = facts();
        let d = job(
            "Business Operations Lead",
            "Requirements\n• Deliver concise reporting on utilization and risk\n• Partner with art directors on pipeline\n• Shape studio culture and morale\n• Own vendor relationships across regions\n• Travel to studios quarterly",
        );
        let b = score(&d, "meets", &[], &f, &Preferences::default());
        let req = b.criteria.iter().find(|c| c.key == "required").unwrap();
        assert!(req.score < 0.6, "one checkable requirement out of five: {}", req.score);
        assert!(b.total < 75, "{}", b.total);
    }

    #[test]
    fn years_count_only_relevant_roles() {
        let f = facts();
        // Smartsheet appears only in the 2020-08 → present role (~6.3 years)
        assert!(matches!(judge("Minimum of 5 years of Smartsheet experience", &f, true, &[]), Verdict::Met(s) if s.contains("about 6.") && s.contains("Business Operations Specialist")));
        assert!(matches!(judge("8+ years of Smartsheet administration", &f, true, &[]), Verdict::Gap(_, _)));
        assert!(matches!(judge("5 years of Salesforce experience", &f, true, &[]), Verdict::Gap(s, false) if s.contains("none of your listed roles")));
        // No field named → all positions count
        assert!(matches!(judge("10+ years of professional experience", &f, true, &[]), Verdict::Met(s) if s.contains("across your listed positions")));
        // People leadership looks at titles and work
        assert!(matches!(judge("Minimum of 2 years of experience managing or training staff", &f, true, &[]), Verdict::Gap(s, false) if s.contains("leading people")));
        assert!(has_evidence("advanced excel and sharepoint", "Microsoft Office"));
    }

    #[test]
    fn levels() {
        assert_eq!(title_level("Senior Operations Manager"), 3.5);
        assert_eq!(title_level("Analytics Coordinator III"), 3.0);
        assert_eq!(title_level("Data Analyst Associate"), 2.0);
        assert_eq!(title_level("Director of Operations"), 4.5);
    }

    #[test]
    fn vocabulary_has_no_duplicates() {
        let mut v: Vec<String> = SKILL_VOCABULARY.iter().map(|s| s.to_lowercase()).collect();
        v.sort();
        let n = v.len();
        v.dedup();
        assert_eq!(n, v.len());
    }
}
