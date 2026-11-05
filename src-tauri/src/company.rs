//! Company research: reads a few public pages of the employer's own website (about, mission,
//! values, careers), follows robots.txt, and pulls out the employer's own language — mission,
//! values, tagline and the phrases it repeats. Nothing about the candidate is sent anywhere:
//! these are plain page requests, made only when the user clicks "Read website".
//!
//! The user chooses which terms to keep. Kept terms reach Claude as citable "W" facts that may
//! describe the employer, never the candidate (see tailoring rule 12).

use crate::db;
use crate::error::{AppError, AppResult};
use crate::parse;
use crate::tailoring::Fact;
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

pub const MAX_PAGES: usize = 6;
/// Recurring phrases offered (and pre-selected) after a read.
const MAX_PHRASES: usize = 16;
const PRESELECT_PHRASES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompanyTerm {
    pub term: String,
    /// mission | vision | value | tagline | phrase | yours
    pub kind: String,
    #[serde(default)]
    pub source_url: String,
    /// The sentence the term was found in.
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageRead {
    pub url: String,
    pub title: String,
    pub chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompanyResearch {
    pub employer: String,
    pub website: String,
    pub pages: Vec<PageRead>,
    pub terms: Vec<CompanyTerm>,
    pub fetched_at: String,
    /// Pages robots.txt asked us not to read, or that failed.
    #[serde(default)]
    pub skipped: Vec<String>,
}

// ───────────────────────────── storage ─────────────────────────────

/// One research record per employer, shared by all its jobs.
pub fn employer_key(e: &str) -> String {
    const STOP: [&str; 8] = ["inc", "llc", "ltd", "corp", "corporation", "co", "company", "the"];
    e.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !STOP.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn get(c: &Connection, employer: &str) -> AppResult<Option<CompanyResearch>> {
    let row: Option<String> = c
        .query_row("SELECT data_json FROM company_research WHERE employer_key=?1", params![employer_key(employer)], |r| r.get(0))
        .optional()?;
    Ok(row.and_then(|s| serde_json::from_str(&s).ok()))
}

pub fn save(c: &Connection, r: &CompanyResearch) -> AppResult<()> {
    c.execute(
        "INSERT INTO company_research (employer_key, employer, website, data_json, updated_at) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(employer_key) DO UPDATE SET employer=excluded.employer, website=excluded.website, data_json=excluded.data_json, updated_at=excluded.updated_at",
        params![employer_key(&r.employer), r.employer, r.website, serde_json::to_string(r).map_err(AppError::other)?, db::now()],
    )?;
    Ok(())
}

/// Saves the user's choice of terms (and any terms they typed themselves).
pub fn save_terms(c: &Connection, employer: &str, terms: Vec<CompanyTerm>) -> AppResult<CompanyResearch> {
    let mut r = get(c, employer)?.unwrap_or_else(|| CompanyResearch { employer: employer.to_string(), ..Default::default() });
    let mut seen = HashSet::new();
    r.terms = terms
        .into_iter()
        .map(|mut t| {
            t.term = t.term.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(400).collect();
            t
        })
        .filter(|t| !t.term.is_empty() && seen.insert((t.kind.clone(), t.term.to_lowercase())))
        .collect();
    save(c, &r)?;
    Ok(r)
}

/// Sites that host other companies' jobs or write about them: never "the company's own website".
const NOT_COMPANY_SITES: [&str; 52] = [
    // applicant tracking systems
    "paylocity", "greenhouse", "lever.co", "myworkdayjobs", "workday", "icims", "smartrecruiters", "jobvite", "ultipro", "ukg",
    "adp.com", "taleo", "oraclecloud", "successfactors", "bamboohr", "applytojob", "recruitee", "ashbyhq", "rippling",
    "dayforcehcm", "breezy.hr", "workable", "jazzhr", "paycomonline", "teamtailor",
    // job boards
    "indeed", "linkedin", "glassdoor", "ziprecruiter", "himalayas", "jobicy", "remoteok", "weworkremotely", "wellfound",
    "builtin", "simplyhired", "careerbuilder", "monster.com", "dice.com", "flexjobs", "remotive", "workingnomads", "usajobs",
    // news and company directories
    "forbes", "bloomberg", "crunchbase", "wikipedia", "techcrunch", "businessinsider", "reuters", "cnbc", "zoominfo",
];

/// Why a web address can't be the company's own site, if it can't.
pub fn not_company_site(host: &str) -> Option<&'static str> {
    let h = host.to_lowercase();
    NOT_COMPANY_SITES.iter().find(|s| h.contains(*s)).copied()
}

/// Words of the employer's name that identify it ("truv", "acme"), skipping generic ones.
pub fn name_tokens(employer: &str) -> Vec<String> {
    const GENERIC: [&str; 24] = [
        "health", "group", "services", "service", "solutions", "technologies", "technology", "systems", "global",
        "international", "resources", "labs", "software", "partners", "holdings", "consulting", "national", "american",
        "united", "first", "and", "for", "of", "the",
    ];
    let key = employer_key(employer);
    let mut t: Vec<String> = key.split(' ').filter(|w| w.len() >= 3 && !GENERIC.contains(w)).map(String::from).collect();
    if t.is_empty() {
        // A name made only of generic words ("Health Resources"): use it whole.
        let whole: String = key.split(' ').collect();
        if whole.len() >= 3 {
            t.push(whole);
        }
    }
    t
}

fn compact(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// The text names the employer (spacing and punctuation ignored: "AcmeCare", "Acme Care", "acmecare.com").
pub fn mentions_employer(text: &str, employer: &str) -> bool {
    let t = compact(text);
    name_tokens(employer).iter().any(|n| t.contains(n.as_str()))
}

/// The employer's homepage, but only when a saved link is on a site named after the employer
/// (never a job board, even one this app doesn't know).
pub fn suggest_website(job: &crate::models::Job) -> Option<String> {
    for u in [&job.draft.posting_url, &job.draft.apply_url].into_iter().flatten() {
        if let Ok(p) = url::Url::parse(u) {
            let host = p.host_str().unwrap_or("").to_lowercase();
            if !host.is_empty() && not_company_site(&host).is_none() && mentions_employer(&host, &job.draft.employer) {
                return Some(format!("{}://{}/", p.scheme(), host));
            }
        }
    }
    None
}

// ───────────────────────────── robots.txt ─────────────────────────────

/// Disallow rules from the `User-agent: *` group (wildcards `*` and `$` supported).
pub fn robots_disallows(txt: &str) -> Vec<String> {
    let mut out = vec![];
    let mut in_star = false;
    let mut last_was_agent = false;
    for raw in txt.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim().to_lowercase(), v.trim());
        if k == "user-agent" {
            if !last_was_agent {
                in_star = false;
            }
            in_star |= v == "*";
            last_was_agent = true;
        } else {
            last_was_agent = false;
            if in_star && k == "disallow" && !v.is_empty() {
                out.push(v.to_string());
            }
        }
    }
    out
}

pub fn robots_allows(rules: &[String], path: &str) -> bool {
    !rules.iter().any(|r| {
        let anchored = r.ends_with('$');
        let body = r.trim_end_matches('$');
        let pat = format!("^{}{}", body.split('*').map(regex::escape).collect::<Vec<_>>().join(".*"), if anchored { "$" } else { "" });
        Regex::new(&pat).map(|re| re.is_match(path)).unwrap_or(false)
    })
}

// ───────────────────────────── link choice ─────────────────────────────

static RE_HREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<a\b[^>]*\bhref\s*=\s*["']([^"'#]+)["']"#).unwrap());

const PAGE_HINTS: [(&str, i32); 16] = [
    ("about", 6), ("who-we-are", 6), ("our-story", 6), ("mission", 6), ("values", 6), ("company", 5), ("culture", 5),
    ("vision", 5), ("purpose", 5), ("careers", 4), ("why-", 3), ("story", 3), ("solutions", 2), ("services", 2), ("what-we-do", 3), ("join", 2),
];
const SKIP_PATHS: [&str; 16] = [
    "feed", "wp-json", "xmlrpc", "login", "portal", "cart", "privacy", "terms", "cookie", "sitemap", "tag/", "category/",
    "events", "contact-old", "/page/", "search",
];

/// Picks up to `MAX_PAGES - 1` same-site pages most likely to describe the company, most useful first.
pub fn choose_pages(base: &url::Url, html: &str, job_words: &[String], rules: &[String]) -> Vec<url::Url> {
    let host = base.host_str().unwrap_or("").trim_start_matches("www.").to_string();
    let mut scored: HashMap<String, (i32, url::Url)> = HashMap::new();
    for cap in RE_HREF.captures_iter(html) {
        let Ok(mut u) = base.join(cap[1].trim()) else { continue };
        u.set_fragment(None);
        if u.query().is_some() || !matches!(u.scheme(), "http" | "https") || u.host_str().unwrap_or("").trim_start_matches("www.") != host {
            continue;
        }
        let path = u.path().to_lowercase();
        if path == "/" || path.is_empty() || SKIP_PATHS.iter().any(|s| path.contains(s)) || Regex::new(r"\.(pdf|png|jpe?g|gif|svg|css|js|xml|zip|docx?)$").unwrap().is_match(&path) {
            continue;
        }
        if !robots_allows(rules, u.path()) {
            continue;
        }
        let mut score: i32 = PAGE_HINTS.iter().filter(|(h, _)| path.contains(h)).map(|(_, s)| *s).sum();
        score += job_words.iter().filter(|w| path.contains(w.as_str())).count() as i32 * 2;
        // Shallow paths are usually the main pages.
        score -= path.trim_matches('/').matches('/').count() as i32 * 2;
        if score <= 0 {
            continue;
        }
        let key = path.trim_end_matches('/').to_string();
        let e = scored.entry(key).or_insert((score, u));
        e.0 = e.0.max(score);
    }
    let mut v: Vec<(i32, url::Url)> = scored.into_values().collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.path().len().cmp(&b.1.path().len())));
    v.into_iter().take(MAX_PAGES - 1).map(|(_, u)| u).collect()
}

/// Distinctive words from the job title ("intake", "member"), used to prefer related pages.
pub fn job_words(title: &str) -> Vec<String> {
    const GENERIC: [&str; 14] = ["manager", "senior", "junior", "lead", "director", "specialist", "coordinator", "associate", "analyst", "assistant", "remote", "hybrid", "of", "and"];
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4 && !GENERIC.contains(w))
        .map(String::from)
        .collect()
}

// ───────────────────────────── text extraction ─────────────────────────────

static RE_CHROME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<(nav|header|footer|form|svg|aside)\b[^>]*>.*?</\s*(?:nav|header|footer|form|svg|aside)\s*>").unwrap());
static RE_HEADING_OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<h([1-6])\b[^>]*>").unwrap());
static RE_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());
static RE_META_DESC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<meta\b[^>]*(?:name|property)\s*=\s*["'](?:description|og:description)["'][^>]*content\s*=\s*["']([^"']+)["']"#).unwrap());

/// A page reduced to lines; heading lines are prefixed with "§<level> ".
pub fn page_lines(html: &str) -> Vec<String> {
    let s = RE_CHROME.replace_all(html, "\n");
    let s = RE_HEADING_OPEN.replace_all(&s, "</p>§$1 ");
    parse::html_to_text(&s)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| l.chars().filter(|c| c.is_alphabetic()).count() >= 2)
        .collect()
}

pub fn title_of(html: &str) -> String {
    RE_TITLE.captures(html).map(|c| parse::decode_entities(c[1].trim())).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn heading(l: &str) -> Option<(u8, &str)> {
    let rest = l.strip_prefix('§')?;
    let lvl = rest.chars().next()?.to_digit(10)? as u8;
    Some((lvl, rest[1..].trim()))
}

fn plain(l: &str) -> &str {
    heading(l).map(|(_, t)| t).unwrap_or(l)
}

static RE_SECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:our\s+)?(mission|vision|purpose|values|core values|guiding principles|what we believe|beliefs)(?:\s+(?:statement|&\s*values))?\s*:?$").unwrap()
});

/// Mission / vision / purpose statements and the list of values, from a page's headed sections.
pub fn statements(lines: &[String], url: &str) -> Vec<CompanyTerm> {
    let mut out = vec![];
    let mut i = 0;
    while i < lines.len() {
        let text = plain(&lines[i]);
        let inline = text.split_once(':').filter(|(h, rest)| RE_SECTION.is_match(h.trim()) && rest.trim().len() > 20);
        let Some(cap) = RE_SECTION.captures(text).or_else(|| inline.and_then(|(h, _)| RE_SECTION.captures(h.trim()))) else {
            i += 1;
            continue;
        };
        let word = cap[1].to_lowercase();
        let kind = if word.contains("value") || word.contains("principle") || word.contains("belie") { "value" } else if word == "purpose" { "mission" } else { word.as_str() };
        if let Some((_, rest)) = inline {
            out.push(term(kind, rest.trim(), url, rest.trim()));
            i += 1;
            continue;
        }
        let mut j = i + 1;
        // The section ends at the next heading of the same or higher rank as its own.
        let own_level = heading(&lines[i]).map(|(l, _)| l).unwrap_or(3);
        if kind == "value" {
            let mut items: Vec<String> = vec![];
            while j < lines.len() && items.len() < 12 {
                if let Some((lvl, _)) = heading(&lines[j]) {
                    if lvl <= own_level && !items.is_empty() {
                        break;
                    }
                }
                let t = plain(&lines[j]);
                if t.len() > 200 {
                    if items.is_empty() { j += 1; continue; }
                    break;
                }
                if t.len() <= 70 {
                    let joins = items.last().map(|p| p.ends_with(',') || p.ends_with('&') || p.to_lowercase().ends_with(" and")).unwrap_or(false)
                        || t.chars().next().map(|c| c.is_lowercase()).unwrap_or(false);
                    match items.last_mut() {
                        Some(p) if joins => { p.push(' '); p.push_str(t); }
                        _ => items.push(t.to_string()),
                    }
                }
                j += 1;
            }
            for it in items {
                let v = it.trim_end_matches([',', '.', ';']).trim().to_string();
                if v.len() >= 3 && !RE_SECTION.is_match(&v) {
                    out.push(term("value", &v, url, &v));
                }
            }
        } else {
            while j < lines.len() && heading(&lines[j]).map(|(l, _)| l >= 4).unwrap_or(false) && plain(&lines[j]).len() < 40 {
                j += 1;
            }
            if let Some(l) = lines.get(j).filter(|l| heading(l).map(|(lv, _)| lv >= 4).unwrap_or(true)) {
                let s = plain(l);
                if s.len() >= 20 {
                    let s: String = s.chars().take(400).collect();
                    out.push(term(kind, &s, url, &s));
                }
            }
        }
        i = j.max(i + 1);
    }
    out
}

fn term(kind: &str, t: &str, url: &str, context: &str) -> CompanyTerm {
    CompanyTerm { term: t.to_string(), kind: kind.into(), source_url: url.into(), context: context.chars().take(300).collect(), selected: true }
}

const STOPWORDS: [&str; 89] = [
    "a", "an", "the", "and", "or", "but", "of", "to", "in", "on", "for", "with", "at", "by", "from", "as", "is", "are", "was",
    "were", "be", "been", "it", "its", "this", "that", "these", "those", "we", "our", "us", "you", "your", "they", "their",
    "who", "what", "which", "how", "all", "any", "more", "most", "other", "some", "such", "can", "will", "may", "into", "over",
    "than", "then", "so", "not", "no", "do", "does", "has", "have", "had", "every", "each", "also", "about", "across", "while",
    "out", "up", "here", "within", "after", "before", "just", "very", "through", "between", "when", "where", "if", "there",
    "them", "i", "my", "me", "he", "she", "his", "her",
];
/// Button and link text ("read the case study", "view the full library") is not company language.
const CTA_STARTS: [&str; 24] = [
    "read", "view", "see", "get", "set", "learn", "download", "watch", "explore", "book", "request", "contact", "start",
    "try", "join", "sign", "click", "find", "discover", "schedule", "browse", "visit", "talk", "call",
];
/// Words addressed to the site's visitor, as in forms and sign-up prompts.
const VISITOR_WORDS: [&str; 12] = ["you", "your", "you're", "you'll", "yours", "please", "enter", "email", "we'll", "what's", "per", "submit"];
/// Words that leave a phrase as a sentence fragment when they start or end it.
const FRAGMENT_EDGES: [&str; 18] = [
    "though", "although", "many", "much", "say", "says", "comes", "next", "do", "does", "did", "let", "lets", "let's",
    "we're", "you're", "they're", "it's",
];
const NOISE: [&str; 28] = [
    "learn more", "read more", "contact us", "click", "privacy", "cookie", "copyright", "rights reserved", "pinterest",
    "facebook", "linkedin", "twitter", "instagram", "youtube", "sign up", "log in", "login", "subscribe", "menu", "skip to",
    "request a demo", "get started", "newsletter", "javascript", "case study", "content library", "book a demo", "free trial",
];

fn words(s: &str) -> Vec<(String, String)> {
    // (lowercase, original)
    s.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’' || c == '-' || c == '&'))
        .map(|w| w.trim_matches(|c: char| c == '\'' || c == '’' || c == '-'))
        .filter(|w| !w.is_empty())
        .map(|w| (w.to_lowercase().replace('’', "'"), w.to_string()))
        .collect()
}

fn sentence_with(text: &str, phrase_lower: &str) -> String {
    text.split(['.', '!', '?', '\n'])
        .find(|s| s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ").contains(phrase_lower))
        .map(|s| s.trim().chars().take(300).collect())
        .unwrap_or_default()
}

/// Phrases of two to four words the site repeats, across pages, ranked by use.
pub fn recurring_phrases(pages: &[(String, Vec<String>)], employer: &str) -> Vec<CompanyTerm> {
    // The company's distinctive name word ("acme"), not every word of its name ("health" stays useful).
    let name_words: HashSet<String> = employer_key(employer).split(' ').filter(|w| w.len() > 2).take(1).map(String::from).collect();
    let mut count: HashMap<String, (usize, HashSet<usize>, String, String, String)> = HashMap::new();
    for (pi, (url, lines)) in pages.iter().enumerate() {
        for l in lines {
            let t = plain(l);
            // Testimonials ("I feel better after...") are customers' words, not the company's.
            let first_person = format!(" {} ", t.to_lowercase()).contains(" i ") || t.to_lowercase().contains(" my ");
            // Questions and form labels ("What's your use case?", "Work email") talk to the visitor.
            let question = t.trim_end().ends_with('?');
            if t.len() < 12 || first_person || question || NOISE.iter().any(|n| t.to_lowercase().contains(n)) {
                continue;
            }
            for seg in t.split([',', ';', ':', '.', '!', '?', '(', ')', '|', '–', '—']) {
            let ws = words(seg);
            for n in 2..=5 {
                for win in ws.windows(n) {
                    let first = &win[0].0;
                    let last = &win[n - 1].0;
                    if STOPWORDS.contains(&first.as_str()) || STOPWORDS.contains(&last.as_str()) || CTA_STARTS.contains(&first.as_str()) || FRAGMENT_EDGES.contains(&first.as_str()) || FRAGMENT_EDGES.contains(&last.as_str()) {
                        continue;
                    }
                    if win.iter().any(|(w, _)| name_words.contains(w) || w.chars().all(|c| c.is_ascii_digit()) || w.len() < 2) {
                        continue;
                    }
                    // "overviewoverview": menu labels run together by the page's markup.
                    let doubled = |w: &str| w.len() >= 6 && w.len() % 2 == 0 && w.is_char_boundary(w.len() / 2) && w[..w.len() / 2] == w[w.len() / 2..];
                    if win.iter().any(|(w, _)| VISITOR_WORDS.contains(&w.as_str()) || doubled(w)) {
                        continue;
                    }
                    if win.iter().filter(|(w, _)| STOPWORDS.contains(&w.as_str())).count() > (n - 1) / 2 {
                        continue;
                    }
                    let key = win.iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>().join(" ");
                    let orig = win.iter().map(|(_, o)| o.as_str()).collect::<Vec<_>>().join(" ");
                    let e = count.entry(key.clone()).or_insert_with(|| (0, HashSet::new(), orig, url.clone(), sentence_with(t, &key)));
                    e.0 += 1;
                    e.1.insert(pi);
                }
            }
            }
        }
    }
    let mut v: Vec<(String, usize, usize, String, String, String)> = count
        .into_iter()
        .filter(|(_, (n, p, ..))| *n >= 2 && (p.len() >= 2 || *n >= 3))
        .map(|(k, (n, p, orig, url, ctx))| (k, n, p.len(), orig, url, ctx))
        .collect();
    // Prefer longer phrases when they are used about as often as their parts.
    let snapshot = v.clone();
    v.retain(|(k, n, ..)| !snapshot.iter().any(|(k2, n2, ..)| k2.len() > k.len() && k2.contains(k.as_str()) && *n2 + 1 >= *n));
    // Language used across several pages is the company's voice; a phrase repeated down one list
    // (an acquisitions timeline, say) is not, so repetition on a page counts for little.
    let rank = |x: &(String, usize, usize, String, String, String)| x.2 * 4 + x.1.min(4) + x.0.matches(' ').count();
    v.sort_by(|a, b| rank(b).cmp(&rank(a)).then(a.0.cmp(&b.0)));
    // Skip phrases that overlap one already kept by three or more words ("feel better after just one").
    let mut kept: Vec<(String, usize, usize, String, String, String)> = vec![];
    for x in v {
        let ws: Vec<&str> = x.0.split(' ').collect();
        let overlaps = ws.len() >= 3 && ws.windows(3).any(|w| { let tri = w.join(" "); kept.iter().any(|k| k.0.contains(&tri)) });
        if !overlaps {
            kept.push(x);
        }
    }
    kept.into_iter()
        .take(MAX_PHRASES)
        .enumerate()
        .map(|(i, (_, _, _, orig, url, ctx))| CompanyTerm { term: tidy_case(&orig), kind: "phrase".into(), source_url: url, context: ctx, selected: i < PRESELECT_PHRASES })
        .collect()
}

/// Lowercases Title-Cased words but keeps acronyms (EAP) as written.
fn tidy_case(s: &str) -> String {
    s.split(' ')
        .map(|w| if w.len() > 1 && w.chars().all(|c| !c.is_alphabetic() || c.is_uppercase()) { w.to_string() } else { w.to_lowercase() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Everything found on the pages read, de-duplicated. Statements first, then the tagline, then phrases.
pub fn extract(employer: &str, home_html: &str, pages: &[(String, String)]) -> Vec<CompanyTerm> {
    let mut terms = vec![];
    let line_pages: Vec<(String, Vec<String>)> = pages.iter().map(|(u, h)| (u.clone(), page_lines(h))).collect();
    for (u, lines) in &line_pages {
        terms.extend(statements(lines, u));
    }
    if let Some(home) = pages.first() {
        if let Some(c) = RE_META_DESC.captures(home_html) {
            let d = parse::decode_entities(c[1].trim());
            if d.len() >= 20 {
                terms.push(term("tagline", &d, &home.0, &d));
            }
        }
    }
    terms.extend(recurring_phrases(&line_pages, employer));
    let mut seen = HashSet::new();
    terms.retain(|t| seen.insert(t.term.to_lowercase()));
    terms
}

/// Keeps the user's own terms and earlier choices when a site is read again.
pub fn merge_previous(mut fresh: Vec<CompanyTerm>, old: &[CompanyTerm]) -> Vec<CompanyTerm> {
    for t in fresh.iter_mut() {
        if let Some(o) = old.iter().find(|o| o.term.eq_ignore_ascii_case(&t.term)) {
            t.selected = o.selected;
        }
    }
    for o in old.iter().filter(|o| o.kind == "yours") {
        if !fresh.iter().any(|t| t.term.eq_ignore_ascii_case(&o.term)) {
            fresh.push(o.clone());
        }
    }
    fresh
}

// ───────────────────────────── fetching ─────────────────────────────

fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("ProjectNextChapter/", env!("CARGO_PKG_VERSION"), " (personal job-search assistant; reads a few public company pages on request)"))
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(AppError::other)
}

async fn get_text(cl: &reqwest::Client, u: &url::Url) -> Result<(url::Url, String), String> {
    let resp = cl.get(u.clone()).send().await.map_err(|e| format!("network error: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let final_url = resp.url().clone();
    let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    if !ct.is_empty() && !ct.contains("html") && !ct.contains("text/plain") {
        return Err(format!("not a web page ({ct})"));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok((final_url, body.chars().take(2_000_000).collect()))
}

/// Reads the homepage and up to five company pages, politely (robots.txt, one request at a time).
pub async fn read_site(website: &str, employer: &str, job_title: &str) -> AppResult<CompanyResearch> {
    let mut site = website.trim().to_string();
    if !site.contains("://") {
        site = format!("https://{site}");
    }
    let base = url::Url::parse(&site).map_err(|_| AppError::invalid("That is not a valid web address"))?;
    if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
        return Err(AppError::invalid("Enter the company's website, for example https://www.example.com"));
    }
    if let Some(site) = not_company_site(base.host_str().unwrap_or("")) {
        return Err(AppError::invalid(format!(
            "{} is a job board or news site ({site}), not {employer}'s own website. Enter the address of {employer}'s own homepage.",
            base.host_str().unwrap_or("")
        )));
    }
    let cl = client()?;
    let mut root = base.clone();
    root.set_path("/");
    root.set_query(None);
    let rules = match root.join("/robots.txt") {
        Ok(r) => match get_text(&cl, &r).await {
            Ok((_, t)) => robots_disallows(&t),
            Err(_) => vec![],
        },
        Err(_) => vec![],
    };
    if !robots_allows(&rules, base.path()) {
        return Err(AppError::invalid("This website's robots.txt asks automated tools not to read that page. Copy the terms you want by hand instead."));
    }
    let (home_url, home) = get_text(&cl, &base).await.map_err(|e| AppError::other(format!("Could not read {base}: {e}")))?;
    let mut pages = vec![(home_url.to_string(), home.clone())];
    let mut skipped = vec![];
    for u in choose_pages(&home_url, &home, &job_words(job_title), &rules) {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        match get_text(&cl, &u).await {
            Ok((fu, h)) => {
                if !pages.iter().any(|(p, _)| p == fu.as_str()) {
                    pages.push((fu.to_string(), h));
                }
            }
            Err(e) => skipped.push(format!("{u} ({e})")),
        }
    }
    // Make sure these pages are about this employer before using a word of them.
    let about_them = pages.iter().filter(|(u, h)| mentions_employer(&format!("{u} {} {}", title_of(h), page_lines(h).join(" ")), employer)).count();
    if about_them == 0 {
        return Err(AppError::invalid(format!(
            "None of the pages read at {} mention {employer}, so this doesn't look like their website. Check the address and try again.",
            home_url.host_str().unwrap_or("")
        )));
    }
    let terms = extract(employer, &home, &pages);
    Ok(CompanyResearch {
        employer: employer.to_string(),
        website: format!("{}://{}/", home_url.scheme(), home_url.host_str().unwrap_or("")),
        pages: pages.iter().map(|(u, h)| PageRead { url: u.clone(), title: title_of(h), chars: page_lines(h).iter().map(|l| l.len()).sum() }).collect(),
        terms,
        fetched_at: db::now(),
        skipped,
    })
}

// ───────────────────────────── prompt facts ─────────────────────────────

fn short_url(u: &str) -> String {
    u.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").trim_end_matches('/').to_string()
}

/// The selected terms as citable "W" facts about the employer.
pub fn facts(r: &CompanyResearch) -> Vec<Fact> {
    let sel: Vec<&CompanyTerm> = r.terms.iter().filter(|t| t.selected && !t.term.trim().is_empty()).collect();
    let mut out = vec![];
    let mut n = 0;
    let mut push = |text: String| {
        n += 1;
        out.push(Fact { id: format!("W{n}"), text });
    };
    for (kind, label) in [("mission", "Mission statement"), ("vision", "Vision statement"), ("tagline", "How the employer describes itself")] {
        for t in sel.iter().filter(|t| t.kind == kind) {
            push(format!("{label} ({}): {}", short_url(&t.source_url), t.term));
        }
    }
    let values: Vec<&str> = sel.iter().filter(|t| t.kind == "value").map(|t| t.term.as_str()).collect();
    if !values.is_empty() {
        push(format!("Stated values: {}", values.join("; ")));
    }
    let phrases: Vec<&str> = sel.iter().filter(|t| t.kind == "phrase" || t.kind == "yours").map(|t| t.term.as_str()).collect();
    if !phrases.is_empty() {
        push(format!("Phrases the employer uses about itself: {}", phrases.join("; ")));
    }
    out
}

/// The `<company_language>` block for the prompt (see tailoring rule 12), or "" when nothing is selected.
pub fn prompt_block(r: &CompanyResearch) -> String {
    let f = facts(r);
    if f.is_empty() {
        return String::new();
    }
    format!(
        "<company_language source=\"{}\">\n{}\n</company_language>\n\n",
        short_url(&r.website),
        f.iter().map(|x| format!("[{}] {}", x.id, x.text)).collect::<Vec<_>>().join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPANY: &str = r#"<html><head><title>Our Story | Acme Care</title>
        <meta name="description" content="Acme Care provides whole health services to help organizations thrive."></head>
        <body><nav><a href="/about/">About</a><a href="/careers/">Careers</a><a href="/feed/">Feed</a>
        <a href="/private/x">Private</a><a href="https://other.com/about">Elsewhere</a></nav>
        <h1>We See Whole Health Differently</h1>
        <p>We drive positive change for organizations, powered by people who care.</p>
        <h2>Our Mission</h2><div><p>Combining compassionate care, expertise, and innovative technologies to unleash the full potential of individuals and organizations.</p></div>
        <h2>Our Values</h2>
        <h5><span>Be helpful</span></h5><p>and caring</p>
        <h5>Be humble,</h5><p>inclusive and collaborative</p>
        <h5>Be open</h5><p>to new possibilities</p>
        <h2>Our History</h2><p>Whole health solutions, powered by people who care, for fifty years and counting across the country.</p>
        <footer>Copyright Acme Care. Learn more about whole health.</footer></body></html>"#;

    const CAREERS: &str = r#"<html><title>Careers</title><body><h1>Join Us</h1>
        <p>We bring whole health benefits to organizations worldwide, powered by people who care.</p>
        <p>Help people unleash their full potential with compassionate care.</p></body></html>"#;

    #[test]
    fn reads_mission_values_tagline_and_phrases() {
        let pages = vec![("https://acme.test/about/".to_string(), COMPANY.to_string()), ("https://acme.test/careers/".to_string(), CAREERS.to_string())];
        let t = extract("Acme Care Inc", COMPANY, &pages);
        let get = |k: &str| t.iter().filter(|x| x.kind == k).map(|x| x.term.clone()).collect::<Vec<_>>();
        assert_eq!(get("mission"), vec!["Combining compassionate care, expertise, and innovative technologies to unleash the full potential of individuals and organizations."]);
        assert_eq!(get("value"), vec!["Be helpful and caring", "Be humble, inclusive and collaborative", "Be open to new possibilities"]);
        assert_eq!(get("tagline").len(), 1);
        let phrases = get("phrase");
        assert!(phrases.contains(&"whole health".to_string()), "{phrases:?}");
        assert!(phrases.contains(&"powered by people who care".to_string()), "{phrases:?}");
        assert!(phrases.iter().any(|p| p.contains("compassionate care")), "{phrases:?}");
        // The company's own name and footer boilerplate are not "buzzwords".
        assert!(!phrases.iter().any(|p| p.contains("acme") || p.contains("learn more")), "{phrases:?}");
    }

    #[test]
    fn robots_rules_are_followed() {
        let r = robots_disallows("User-agent: Googlebot\nDisallow: /x\n\nUser-agent: *\nDisallow: /private/\nDisallow: /*feed\nDisallow: /exact$\n");
        assert_eq!(r, vec!["/private/", "/*feed", "/exact$"]);
        assert!(!robots_allows(&r, "/private/x"));
        assert!(!robots_allows(&r, "/blog/feed/"));
        assert!(!robots_allows(&r, "/exact"));
        assert!(robots_allows(&r, "/exact/more"));
        assert!(robots_allows(&r, "/about/"));
    }

    #[test]
    fn chooses_same_site_company_pages_only() {
        let base = url::Url::parse("https://acme.test/").unwrap();
        let rules = robots_disallows("User-agent: *\nDisallow: /private/");
        let p: Vec<String> = choose_pages(&base, COMPANY, &[], &rules).iter().map(|u| u.path().to_string()).collect();
        assert_eq!(p, vec!["/about/", "/careers/"]);
    }

    #[test]
    fn only_selected_terms_become_facts() {
        let mut r = CompanyResearch { employer: "Acme".into(), website: "https://www.acme.test/".into(), ..Default::default() };
        r.terms = vec![
            term("mission", "Help people thrive.", "https://acme.test/about/", ""),
            term("value", "Be kind", "", ""),
            term("value", "Be bold", "", ""),
            CompanyTerm { selected: false, ..term("phrase", "synergy", "", "") },
            term("yours", "whole health", "", ""),
        ];
        let f = facts(&r);
        assert_eq!(f.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["W1", "W2", "W3"]);
        assert_eq!(f[1].text, "Stated values: Be kind; Be bold");
        assert!(f[2].text.ends_with("whole health"));
        let b = prompt_block(&r);
        assert!(b.starts_with("<company_language source=\"acme.test\">") && !b.contains("synergy"));
        r.terms.iter_mut().for_each(|t| t.selected = false);
        assert_eq!(prompt_block(&r), "");
    }

    #[test]
    fn suggests_employer_site_but_not_job_boards() {
        let mut j = crate::tailoring::tests::job();
        j.draft.posting_url = Some("https://recruiting.paylocity.com/Recruiting/Jobs/Details/1".into());
        j.draft.apply_url = None;
        assert_eq!(suggest_website(&j), None);
        j.draft.employer = "Acme Care".into();
        j.draft.apply_url = Some("https://careers.acme.test/jobs/9".into());
        assert_eq!(suggest_website(&j).as_deref(), Some("https://careers.acme.test/"));
        assert_eq!(employer_key("Acme Health Resources, Inc."), "acme health resources");
        // Job boards this app has never heard of are not suggested either: the host must carry the employer's name.
        j.draft.employer = "Truv".into();
        j.draft.posting_url = Some("https://himalayas.app/companies/truv/jobs/technical-support-engineer".into());
        j.draft.apply_url = Some("https://some-new-board.io/jobs/truv-support".into());
        assert_eq!(suggest_website(&j), None);
        j.draft.apply_url = Some("https://www.truv.com/careers/support".into());
        assert_eq!(suggest_website(&j).as_deref(), Some("https://www.truv.com/"));
    }

    #[test]
    fn buttons_forms_and_menus_are_not_company_language() {
        let page = |body: &str| format!("<html><title>Truv</title><body>{body}</body></html>");
        let a = page("<p>Read the case study about bank income verification.</p><p>What's your use case?</p><p>Work email please enter your address now.</p><p>Auto lending overviewoverview menu items here.</p><p>Bank income verification for lenders.</p>");
        let b = page("<p>Read the case study about bank income verification.</p><p>What's your use case?</p><p>Work email please enter your address now.</p><p>Auto lending overviewoverview menu items here.</p><p>Faster bank income verification today.</p>");
        let pages = vec![("https://truv.test/".to_string(), a), ("https://truv.test/about".to_string(), b)];
        let found: Vec<String> = recurring_phrases(&pages.iter().map(|(u, h)| (u.clone(), page_lines(h))).collect::<Vec<_>>(), "Truv").into_iter().map(|t| t.term).collect();
        assert!(found.contains(&"bank income verification".to_string()), "{found:?}");
        for junk in ["read the case", "case study", "use case", "work email", "please enter", "overviewoverview"] {
            assert!(!found.iter().any(|f| f.contains(junk)), "{junk} in {found:?}");
        }
    }

    #[test]
    fn job_boards_and_news_sites_are_not_company_websites() {
        for h in ["himalayas.app", "jobicy.com", "www.forbes.com", "www.linkedin.com", "boards.greenhouse.io", "jobs.dayforcehcm.com"] {
            assert!(not_company_site(h).is_some(), "{h}");
        }
        for h in ["acmecare.test", "www.truv.com", "starlink.com", "maze.co"] {
            assert!(not_company_site(h).is_none(), "{h}");
        }
    }

    #[test]
    fn pages_must_name_the_employer() {
        assert_eq!(name_tokens("AcmeCare Health Resources Inc"), vec!["acmecare"]);
        assert_eq!(name_tokens("Health Resources"), vec!["healthresources"]);
        assert!(mentions_employer("Welcome to Acme Care Health", "AcmeCare Health Resources Inc"));
        assert!(mentions_employer("https://www.truv.com/ Truv | Income verification", "Truv"));
        assert!(!mentions_employer("Himalayas is the leading remote job board", "Fortive"));
        // Generic words alone don't count as a mention.
        assert!(!mentions_employer("Your health resources for remote jobs", "Acme Health Resources"));
    }
}
