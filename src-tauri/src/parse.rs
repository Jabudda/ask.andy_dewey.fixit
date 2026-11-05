//! Turning job postings (structured JSON-LD or free text) into editable drafts.
//!
//! Every inference is conservative and leaves a note in `detection_notes`; when the text
//! does not say something, the value stays `unknown` instead of being guessed.

use crate::models::JobDraft;
use chrono::NaiveDate;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

pub const US_STATES: [&str; 50] = [
    "Alabama", "Alaska", "Arizona", "Arkansas", "California", "Colorado", "Connecticut",
    "Delaware", "Florida", "Georgia", "Hawaii", "Idaho", "Illinois", "Indiana", "Iowa",
    "Kansas", "Kentucky", "Louisiana", "Maine", "Maryland", "Massachusetts", "Michigan",
    "Minnesota", "Mississippi", "Missouri", "Montana", "Nebraska", "Nevada", "New Hampshire",
    "New Jersey", "New Mexico", "New York", "North Carolina", "North Dakota", "Ohio",
    "Oklahoma", "Oregon", "Pennsylvania", "Rhode Island", "South Carolina", "South Dakota",
    "Tennessee", "Texas", "Utah", "Vermont", "Virginia", "Washington", "West Virginia",
    "Wisconsin", "Wyoming",
];

fn re(p: &str) -> Regex {
    Regex::new(p).expect("valid regex")
}

const US: &str = r"(?:u\.s\.(?:a\.)?|usa?\b|united states)";

static RE_NOT_REMOTE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:not (?:a )?remote|no remote|remote work is not (?:available|offered))"));
static RE_HYBRID: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\bhybrid\b|\b\d\s*(?:days?|x)\s*(?:per|a|/)\s*week\s+(?:in|on)[- ]?(?:the\s+)?(?:office|site)|\bin[- ]office\s+\d")
});
static RE_ONSITE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:on[- ]?site|in[- ]person|in the office|office[- ]based)\b"));
static RE_REMOTE: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)(?:\b(?:fully remote|remote|work from home|wfh|telecommute|telework|work from anywhere)\b|100% remote)")
});

static RE_WI_EXCLUDED: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?is)(?:\bnot|unable to|cannot|can't|can not)\s+(?:currently\s+)?(?:able to\s+)?(?:hire|employ|accept|consider|support)[^.]{0,200}?\bwisconsin\b|(?:excluding|except(?:\s+for)?|with the exception of)[^.]{0,150}?\bwisconsin\b|\bwisconsin\b[^.]{0,60}?\b(?:excluded|not eligible|ineligible)")
});
static RE_WI_NAME: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bwisconsin\b"));
static RE_WI_ABBR: LazyLock<Regex> = LazyLock::new(|| re(r",\s*WI\b|\bWI\s+\d{5}\b|\(WI\)"));
static RE_STATE_LIST_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)(?:following states|residents of|reside in|eligible states|able to hire in|currently hiring in|hire in the following|located in one of|states:)")
});
static RE_US_WIDE: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"(?i)(?:anywhere in the {US}|remote\s*[(,\-–:]\s*{US}|all 50 (?:u\.s\.\s*|us\s+)?states|\bu\.?s\.?[- ]based\b|within the {US}|\b{US}[- ]remote|remote (?:with)?in the {US})"
    ))
});

static RE_SALARY: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\$\s?(\d{1,3}(?:,\d{3})+|\d+(?:\.\d{1,2})?)\s?(k)?(?:\s*(?:-|–|—|to)\s*\$?\s?(\d{1,3}(?:,\d{3})+|\d+(?:\.\d{1,2})?)\s?(k)?)?")
});
static RE_NOT_SALARY: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\s*(?:m|mm|b|bn|million|billion)\b"));
static RE_HOURLY: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(?:/\s*h(?:ou)?r\b|\bper\s+hour\b|\ban?\s+hour\b|\bhourly\b|\bhr\b)"));
static RE_YEARLY: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(?:/\s*y(?:ea)?r\b|\bper\s+year\b|\ba\s+year\b|\bannual(?:ly)?\b|\bper\s+annum\b|\byearly\b|\bsalary\b)"));
static RE_MONTHLY: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(?:/\s*mo(?:nth)?\b|\bper\s+month\b|\bmonthly\b)"));

static RE_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?im)^\s*(job title|position title|title|position|role|company|employer|organization|hiring organization|location|apply(?: at| url| link| here)?|application (?:url|link)|url|link|posted|date posted|posting date)\s*[:\-–]\s*(.+?)\s*$")
});
static RE_URL: LazyLock<Regex> = LazyLock::new(|| re(r#"https?://[^\s<>"')\]]+"#));

static RE_LDJSON: LazyLock<Regex> = LazyLock::new(|| {
    re(r#"(?is)<script[^>]*type\s*=\s*["']application/ld\+json["'][^>]*>(.*?)</script>"#)
});
static RE_SCRIPT_STYLE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?is)<script\b.*?</script>|<style\b.*?</style>|<noscript\b.*?</noscript>"));
static RE_BLOCK_END: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)<\s*(?:br\s*/?|/p|/div|/h[1-6]|/tr|/ul|/ol|/section)\s*>"));
static RE_LI: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)<\s*li\b[^>]*>"));
static RE_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)<[^>]+>"));
static RE_NUM_ENTITY: LazyLock<Regex> = LazyLock::new(|| re(r"&#(x[0-9a-fA-F]+|\d+);"));
static RE_TITLE_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?is)<title[^>]*>(.*?)</title>"));
static RE_MULTI_NL: LazyLock<Regex> = LazyLock::new(|| re(r"\n{3,}"));

// ───────────────────────────── text helpers ─────────────────────────────

pub fn decode_entities(s: &str) -> String {
    let s = RE_NUM_ENTITY.replace_all(s, |c: &regex::Captures| {
        let raw = &c[1];
        let code = if let Some(hex) = raw.strip_prefix('x') {
            u32::from_str_radix(hex, 16).ok()
        } else {
            raw.parse::<u32>().ok()
        };
        code.and_then(char::from_u32).map(|ch| ch.to_string()).unwrap_or_default()
    });
    s.replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&rsquo;", "’")
        .replace("&lsquo;", "‘")
        .replace("&ndash;", "–")
        .replace("&mdash;", "—")
        .replace("&bull;", "•")
        .replace("&amp;", "&")
}

/// Convert an HTML fragment (possibly entity-escaped, as in many JSON-LD feeds) to readable text.
pub fn html_to_text(html: &str) -> String {
    let src = if html.contains("&lt;") && !html.contains('<') {
        decode_entities(html)
    } else {
        html.to_string()
    };
    let s = RE_SCRIPT_STYLE.replace_all(&src, "");
    let s = RE_LI.replace_all(&s, "\n• ");
    let s = RE_BLOCK_END.replace_all(&s, "\n");
    let s = RE_TAG.replace_all(&s, "");
    let s = decode_entities(&s);
    let lines: Vec<String> = s.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    let joined = lines.join("\n");
    RE_MULTI_NL.replace_all(joined.trim(), "\n\n").to_string()
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Stable identity for duplicate detection: normalized employer (legal suffixes removed) + title.
pub fn fingerprint(employer: &str, title: &str) -> String {
    const SUFFIXES: [&str; 10] = ["inc", "llc", "corp", "corporation", "co", "ltd", "company", "plc", "lp", "the"];
    let emp: Vec<String> = normalize(employer)
        .split(' ')
        .filter(|w| !SUFFIXES.contains(w))
        .map(String::from)
        .collect();
    format!("{}|{}", emp.join(" "), normalize(title))
}

/// URL without tracking parameters, fragment or trailing slash — used to catch the same posting twice.
pub fn canonical_url(raw: &str) -> Option<String> {
    let mut u = url::Url::parse(raw.trim()).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    u.set_fragment(None);
    let kept: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| {
            let k = k.to_lowercase();
            !(k.starts_with("utm_") || ["source", "ref", "referrer", "gh_src", "lever-source", "src", "trk"].contains(&k.as_str()))
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        u.set_query(None);
    } else {
        u.query_pairs_mut().clear().extend_pairs(kept);
    }
    let host = u.host_str()?.trim_start_matches("www.").to_lowercase();
    let path = u.path().trim_end_matches('/').to_string();
    Some(match u.query() {
        Some(q) => format!("{host}{path}?{q}"),
        None => format!("{host}{path}"),
    })
}

/// Re-joins lines that a PDF wrapped mid-sentence, keeping bullets, headings and paragraphs.
pub fn unwrap_text(t: &str) -> String {
    const BULLETS: [char; 8] = ['•', '-', '*', '·', '–', '●', '▪', '◦'];
    let mut out: Vec<String> = vec![];
    for line in t.lines().map(str::trim) {
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        if let Some(prev) = out.last_mut() {
            let p = prev.trim_end();
            let first = line.chars().next().unwrap_or(' ');
            let continues = !p.is_empty()
                && !line.starts_with(BULLETS)
                && !p.ends_with(['.', ':', ';', '!', '?'])
                && (first.is_lowercase()
                    || first.is_ascii_digit()
                    || matches!(first, '(' | '&' | '+')
                    || p.ends_with([',', '-', '/', '&', '('])
                    || p.split_whitespace().count() >= 6);
            if continues {
                prev.push(' ');
                prev.push_str(line);
                continue;
            }
        }
        out.push(line.to_string());
    }
    RE_MULTI_NL.replace_all(out.join("\n").trim(), "\n\n").to_string()
}

pub fn normalize_date(raw: &str) -> Option<String> {
    let t = raw.trim();
    let head = t.get(..10).unwrap_or(t);
    if let Ok(d) = NaiveDate::parse_from_str(head, "%Y-%m-%d") {
        return Some(d.to_string());
    }
    for f in ["%m/%d/%Y", "%B %d, %Y", "%b %d, %Y", "%d %B %Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(t, f) {
            return Some(d.to_string());
        }
    }
    None
}

// ───────────────────────────── detectors ─────────────────────────────

pub fn detect_remote(text: &str) -> (String, Option<String>) {
    if RE_NOT_REMOTE.is_match(text) {
        return ("onsite".into(), Some("Posting states the role is not remote".into()));
    }
    if RE_HYBRID.is_match(text) {
        return ("hybrid".into(), Some("Posting mentions hybrid work or scheduled office days".into()));
    }
    match (RE_REMOTE.is_match(text), RE_ONSITE.is_match(text)) {
        (true, true) => (
            "remote".into(),
            Some("Mentions both remote and on-site/in-person work — confirm there is no routine office attendance".into()),
        ),
        (true, false) => ("remote".into(), Some("Remote work stated in the posting text".into())),
        (false, true) => ("onsite".into(), Some("Posting describes on-site / in-person work".into())),
        (false, false) => ("unknown".into(), None),
    }
}

pub fn detect_wisconsin(text: &str) -> (String, Option<String>) {
    if RE_WI_EXCLUDED.is_match(text) {
        return ("excluded".into(), Some("Posting indicates Wisconsin residents are not eligible".into()));
    }
    if RE_WI_NAME.is_match(text) || RE_WI_ABBR.is_match(text) {
        return ("eligible".into(), Some("Wisconsin is named in the posting — verify the context".into()));
    }
    for m in RE_STATE_LIST_MARKER.find_iter(text) {
        let window: String = text[m.end()..].chars().take(600).collect();
        let found: Vec<&str> = US_STATES
            .iter()
            .copied()
            .filter(|st| Regex::new(&format!(r"(?i)\b{}\b", regex::escape(st))).map(|r| r.is_match(&window)).unwrap_or(false))
            .collect();
        if found.len() >= 2 {
            return (
                "excluded".into(),
                Some(format!("Eligible states listed ({}) do not include Wisconsin", found.join(", "))),
            );
        }
    }
    if RE_US_WIDE.is_match(text) {
        return ("eligible".into(), Some("Posting states US-wide remote eligibility".into()));
    }
    ("unknown".into(), None)
}

/// Returns (min, max, period). Ignores dollar amounts followed by "million"/"billion".
pub fn parse_salary(text: &str) -> Option<(f64, f64, String)> {
    let num = |s: &str| s.replace(',', "").parse::<f64>().ok();
    for c in RE_SALARY.captures_iter(text) {
        let whole = c.get(0)?;
        let after: String = text[whole.end()..].chars().take(30).collect();
        if RE_NOT_SALARY.is_match(&after) {
            continue;
        }
        let before: String = {
            let b: Vec<char> = text[..whole.start()].chars().collect();
            b[b.len().saturating_sub(40)..].iter().collect()
        };
        let lo_k = c.get(2).is_some();
        let hi_k = c.get(4).is_some();
        let mut lo = num(&c[1])?;
        let mut hi = c.get(3).and_then(|m| num(m.as_str()));
        if lo_k {
            lo *= 1000.0;
        }
        if let Some(h) = hi.as_mut() {
            if hi_k {
                *h *= 1000.0;
                if !lo_k && lo < 1000.0 {
                    lo *= 1000.0; // "$50-60k"
                }
            }
        }
        if lo < 7.0 {
            continue;
        }
        let hi = hi.unwrap_or(lo).max(lo);
        // Context words before the amount (e.g. "Hourly rate:") are weaker evidence than words after
        // it, and an "HR" mention nearby must not turn $70,000 into an hourly rate.
        let plausible_hourly = hi < 500.0;
        let period = if plausible_hourly && (RE_HOURLY.is_match(&after) || RE_HOURLY.is_match(&before)) {
            "hour"
        } else if RE_MONTHLY.is_match(&after) && hi < 50_000.0 {
            "month"
        } else if !plausible_hourly || RE_YEARLY.is_match(&after) || RE_YEARLY.is_match(&before) {
            "year"
        } else {
            "hour"
        };
        return Some((lo, hi, period.into()));
    }
    None
}

pub fn detect_employment_type(text: &str) -> String {
    let checks: [(&str, &str); 4] = [
        (r"(?i)\bfull[- ]?time\b", "Full-time"),
        (r"(?i)\bpart[- ]?time\b", "Part-time"),
        (r"(?i)\b(?:contract|contractor|1099|c2c|corp[- ]to[- ]corp)\b", "Contract"),
        (r"(?i)\b(?:temporary|temp[- ]to[- ]hire|seasonal|fixed[- ]term)\b", "Temporary"),
    ];
    checks
        .iter()
        .filter(|(p, _)| re(p).is_match(text))
        .map(|(_, l)| *l)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Fill any still-unknown fields from the title/location/description text.
pub fn enrich(d: &mut JobDraft) {
    let text = format!("{}\n{}\n{}", d.title, d.location_text, d.description);
    if d.remote_type.is_empty() || d.remote_type == "unknown" {
        let (t, note) = detect_remote(&text);
        d.remote_type = t;
        d.detection_notes.extend(note);
    }
    if d.wisconsin_eligibility.is_empty() || d.wisconsin_eligibility == "unknown" {
        let (w, note) = detect_wisconsin(&text);
        d.wisconsin_eligibility = w;
        d.detection_notes.extend(note);
    }
    if d.salary_min.is_none() && d.salary_max.is_none() {
        if let Some((lo, hi, p)) = parse_salary(&d.description) {
            d.salary_min = Some(lo);
            d.salary_max = Some(hi);
            d.detection_notes.push(format!("Compensation read from description text (per {p}) — verify"));
            d.salary_period = Some(p);
        }
    }
    if d.employment_type.is_empty() {
        d.employment_type = detect_employment_type(&text);
    }
}

// ───────────────────────────── pasted text ─────────────────────────────

pub fn parse_pasted(text: &str) -> JobDraft {
    let mut d = JobDraft {
        source: "manual_paste".into(),
        extraction_method: "heuristic".into(),
        description: text.trim().to_string(),
        ..Default::default()
    };
    for c in RE_FIELD.captures_iter(text) {
        let key = c[1].to_lowercase();
        let val = c[2].trim().to_string();
        match key.as_str() {
            "job title" | "position title" | "title" | "position" | "role" if d.title.is_empty() => d.title = val,
            "company" | "employer" | "organization" | "hiring organization" if d.employer.is_empty() => d.employer = val,
            "location" if d.location_text.is_empty() => d.location_text = val,
            "posted" | "date posted" | "posting date" if d.posted_date.is_none() => d.posted_date = normalize_date(&val),
            k if (k.starts_with("apply") || k.starts_with("application") || k == "url" || k == "link") && d.apply_url.is_none() => {
                d.apply_url = RE_URL.find(&val).map(|m| m.as_str().to_string());
            }
            _ => {}
        }
    }
    if d.apply_url.is_none() {
        d.apply_url = RE_URL.find(text).map(|m| m.as_str().trim_end_matches(['.', ',']).to_string());
    }
    d.posting_url = d.apply_url.clone();
    if d.title.is_empty() {
        if let Some(first) = text.lines().map(str::trim).find(|l| !l.is_empty() && l.chars().count() <= 120) {
            d.title = first.to_string();
            d.detection_notes.push("Job title guessed from the first line — verify".into());
        }
    }
    if d.employer.is_empty() {
        d.detection_notes.push("Employer not detected — enter it before saving".into());
    }
    enrich(&mut d);
    d
}

// ───────────────────────────── JSON-LD (schema.org JobPosting) ─────────────────────────────

pub fn find_jobposting_jsonld(html: &str) -> Option<Value> {
    fn is_job(t: Option<&Value>) -> bool {
        match t {
            Some(Value::String(s)) => s == "JobPosting",
            Some(Value::Array(a)) => a.iter().any(|x| x.as_str() == Some("JobPosting")),
            _ => false,
        }
    }
    fn walk(v: &Value) -> Option<&Value> {
        match v {
            Value::Array(a) => a.iter().find_map(walk),
            Value::Object(o) => {
                if is_job(o.get("@type")) {
                    Some(v)
                } else {
                    o.get("@graph").and_then(walk)
                }
            }
            _ => None,
        }
    }
    RE_LDJSON.captures_iter(html).find_map(|c| {
        let raw = c[1].trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>");
        serde_json::from_str::<Value>(raw).ok().and_then(|v| walk(&v).cloned())
    })
}

fn sval(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        Value::Object(o) => sval(o.get("name")),
        _ => None,
    }
}

fn fval(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.replace([',', '$'], "").trim().parse().ok(),
        _ => None,
    }
}

fn as_list(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(x) => vec![x],
        None => vec![],
    }
}

pub fn draft_from_jsonld(v: &Value, page_url: &str) -> JobDraft {
    let mut d = JobDraft {
        source: "manual_url".into(),
        extraction_method: "json-ld".into(),
        ..Default::default()
    };
    d.detection_notes.push("Read from the page's schema.org JobPosting structured data".into());
    d.title = sval(v.get("title")).map(|t| decode_entities(&t)).unwrap_or_default();
    d.employer = sval(v.get("hiringOrganization")).map(|t| decode_entities(&t)).unwrap_or_default();
    d.description = v.get("description").and_then(Value::as_str).map(html_to_text).unwrap_or_default();
    d.posted_date = sval(v.get("datePosted")).and_then(|s| normalize_date(&s));
    d.valid_through = sval(v.get("validThrough")).and_then(|s| normalize_date(&s));
    d.posting_url = Some(page_url.to_string());
    d.apply_url = sval(v.get("url")).filter(|u| u.starts_with("http")).or_else(|| Some(page_url.to_string()));

    let locs: Vec<String> = as_list(v.get("jobLocation"))
        .into_iter()
        .filter_map(|l| {
            let a = l.get("address")?;
            if let Some(s) = a.as_str() {
                return Some(s.to_string());
            }
            let parts: Vec<String> = ["addressLocality", "addressRegion", "addressCountry"]
                .iter()
                .filter_map(|k| sval(a.get(*k)))
                .collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        })
        .collect();
    d.location_text = locs.join(" · ");

    let telecommute = as_list(v.get("jobLocationType"))
        .iter()
        .any(|t| t.as_str().map(|s| s.eq_ignore_ascii_case("TELECOMMUTE")).unwrap_or(false));
    if telecommute {
        d.remote_type = "remote".into();
        d.detection_notes.push("Structured data marks the job as TELECOMMUTE (remote)".into());
        if d.location_text.is_empty() {
            d.location_text = "Remote".into();
        }
    }

    let names: Vec<String> = as_list(v.get("applicantLocationRequirements"))
        .into_iter()
        .filter_map(|x| sval(Some(x)))
        .collect();
    if !names.is_empty() {
        let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
        let us = ["us", "usa", "u.s.", "united states", "united states of america"];
        if lower.iter().any(|n| n.contains("wisconsin")) {
            d.wisconsin_eligibility = "eligible".into();
            d.detection_notes.push("Applicant location requirements include Wisconsin".into());
        } else if lower.iter().any(|n| us.contains(&n.as_str())) {
            d.wisconsin_eligibility = "eligible".into();
            d.detection_notes.push("Applicant location requirement: United States".into());
        } else {
            d.wisconsin_eligibility = "excluded".into();
            d.detection_notes.push(format!("Applicant location requirements ({}) do not include Wisconsin", names.join(", ")));
        }
    }

    if let Some(bs) = v.get("baseSalary") {
        let val = bs.get("value").unwrap_or(bs);
        let single = fval(val.get("value")).or_else(|| fval(Some(val)));
        let lo = fval(val.get("minValue")).or(single);
        let hi = fval(val.get("maxValue")).or(lo);
        if lo.is_some() || hi.is_some() {
            d.salary_min = lo.or(hi);
            d.salary_max = hi.or(lo);
            let unit = sval(val.get("unitText")).or_else(|| sval(bs.get("unitText"))).unwrap_or_default().to_uppercase();
            d.salary_period = Some(
                match unit.as_str() {
                    "HOUR" => "hour",
                    "DAY" => "day",
                    "WEEK" => "week",
                    "MONTH" => "month",
                    _ => "year",
                }
                .into(),
            );
            d.detection_notes.push("Compensation read from structured baseSalary".into());
        }
    }

    let types: Vec<&str> = as_list(v.get("employmentType"))
        .into_iter()
        .filter_map(Value::as_str)
        .filter_map(|t| match t.to_uppercase().replace(['-', ' '], "_").as_str() {
            "FULL_TIME" => Some("Full-time"),
            "PART_TIME" => Some("Part-time"),
            "CONTRACTOR" | "CONTRACT" => Some("Contract"),
            "TEMPORARY" | "TEMP" => Some("Temporary"),
            _ => None,
        })
        .collect();
    d.employment_type = types.join(", ");

    enrich(&mut d);
    d
}

/// Fallback when a page has no structured data: page title + visible text, flagged for review.
static RE_COMPANY: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:inc|llc|ltd|corp|corporation|co|company|group|plc|lp|careers|jobs|recruiting)\.?$"));

/// "Acme Inc - Data Analyst" / "Data Analyst | Acme" → (title, employer, confident?)
pub fn split_page_title(raw: &str) -> (String, String, bool) {
    let clean = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let parts: Vec<&str> = clean.split(" - ").flat_map(|p| p.split(" | ")).flat_map(|p| p.split(" — ")).map(str::trim).filter(|p| !p.is_empty()).collect();
    match parts.len() {
        0 => (String::new(), String::new(), false),
        1 => (parts[0].to_string(), String::new(), false),
        _ => {
            let parts: Vec<&str> = parts.into_iter().filter(|p| !p.eq_ignore_ascii_case("careers") && !p.eq_ignore_ascii_case("jobs")).collect();
            if let Some(i) = parts.iter().position(|p| RE_COMPANY.is_match(p)) {
                let title = parts.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, p)| *p).collect::<Vec<_>>().join(" - ");
                (title, parts[i].trim_end_matches(" Careers").to_string(), true)
            } else {
                (parts[0].to_string(), parts.last().map(|s| s.to_string()).unwrap_or_default(), false)
            }
        }
    }
}

pub fn draft_from_html_fallback(html: &str, page_url: &str) -> JobDraft {
    let page_title = RE_TITLE_TAG
        .captures(html)
        .map(|c| html_to_text(&c[1]))
        .unwrap_or_default();
    let (title, employer, confident) = split_page_title(&page_title);
    let body = html_to_text(html);
    let mut d = JobDraft {
        source: "manual_url".into(),
        extraction_method: "heuristic".into(),
        title,
        employer: employer.clone(),
        description: body.chars().take(20_000).collect(),
        posting_url: Some(page_url.to_string()),
        apply_url: Some(page_url.to_string()),
        ..Default::default()
    };
    d.detection_notes.push(
        "No structured job data on this page. Title and employer come from the page title and the description is raw page text — \
         many career sites load postings with JavaScript, so paste the description instead if it looks incomplete."
            .into(),
    );
    if employer.is_empty() {
        d.detection_notes.push("Employer not detected — enter it before saving".into());
    } else if !confident {
        d.detection_notes.push(format!("Title “{}” and employer “{employer}” were guessed from the page title “{page_title}” — verify", d.title));
    }
    enrich(&mut d);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_detection() {
        assert_eq!(detect_remote("This is a fully remote role").0, "remote");
        assert_eq!(detect_remote("Hybrid schedule in Chicago").0, "hybrid");
        assert_eq!(detect_remote("Remote-friendly, 3 days per week in office").0, "hybrid");
        assert_eq!(detect_remote("This position is not remote.").0, "onsite");
        assert_eq!(detect_remote("Work on-site at our plant").0, "onsite");
        assert_eq!(detect_remote("100% remote").0, "remote");
        let (t, note) = detect_remote("Remote, with occasional in-person team events");
        assert_eq!(t, "remote");
        assert!(note.unwrap().contains("confirm"));
        assert_eq!(detect_remote("Great benefits").0, "unknown");
    }

    #[test]
    fn wisconsin_detection() {
        assert_eq!(detect_wisconsin("We are unable to hire in Wisconsin, Alaska or Hawaii.").0, "excluded");
        assert_eq!(detect_wisconsin("Remote anywhere in the US excluding Wisconsin and Ohio").0, "excluded");
        assert_eq!(
            detect_wisconsin("Candidates must reside in one of the following states: Texas, Florida, Georgia, Arizona.").0,
            "excluded"
        );
        assert_eq!(
            detect_wisconsin("We can hire in the following states: Illinois, Wisconsin, Minnesota").0,
            "eligible"
        );
        assert_eq!(detect_wisconsin("Remote (US) — open to candidates anywhere").0, "eligible");
        assert_eq!(detect_wisconsin("Open to U.S.-based candidates").0, "eligible");
        assert_eq!(detect_wisconsin("Location: Milwaukee, WI 53202").0, "eligible");
        assert_eq!(detect_wisconsin("Remote role, great team").0, "unknown");
        // "WI-FI" style text must not be read as Wisconsin
        assert_eq!(detect_wisconsin("Requires reliable Wi-Fi").0, "unknown");
    }

    #[test]
    fn salary_parsing() {
        assert_eq!(parse_salary("Salary: $65,000 - $80,000 per year"), Some((65000.0, 80000.0, "year".into())));
        assert_eq!(parse_salary("Pay range $28 to $34/hr"), Some((28.0, 34.0, "hour".into())));
        assert_eq!(parse_salary("Compensation $50-60k"), Some((50000.0, 60000.0, "year".into())));
        assert_eq!(parse_salary("Hourly rate: $45"), Some((45.0, 45.0, "hour".into())));
        assert_eq!(parse_salary("We raised $40 million in funding. Salary $72,000"), Some((72000.0, 72000.0, "year".into())));
        assert_eq!(parse_salary("Competitive pay"), None);
        // An "HR" mention must not turn an annual salary into an hourly rate.
        assert_eq!(parse_salary("Questions? Contact HR. Salary $70,000"), Some((70000.0, 70000.0, "year".into())));
    }

    #[test]
    fn employment_types() {
        assert_eq!(detect_employment_type("Full-time, permanent"), "Full-time");
        assert_eq!(detect_employment_type("6-month contract, temp-to-hire"), "Contract, Temporary");
    }

    #[test]
    fn fingerprints_and_urls() {
        assert_eq!(fingerprint("Acme, Inc.", "Data Analyst II"), fingerprint("ACME", "data analyst ii"));
        assert_ne!(fingerprint("Acme", "Data Analyst"), fingerprint("Acme", "Data Engineer"));
        assert_eq!(
            canonical_url("https://www.Example.com/jobs/123/?utm_source=x&gh_src=abc#apply"),
            Some("example.com/jobs/123".into())
        );
        assert_eq!(
            canonical_url("https://boards.example.com/job?id=9&utm_medium=email"),
            Some("boards.example.com/job?id=9".into())
        );
        assert_eq!(canonical_url("ftp://example.com/x"), None);
    }

    #[test]
    fn unwraps_pdf_lines() {
        let t = "What you'll need\nCustomer-facing program experience.\n4+\nyears of relevant experience in program\nmanagement, implementation, customer\nsuccess or consulting.\n• Excel\n• Smartsheet";
        let u = unwrap_text(t);
        assert_eq!(
            u,
            "What you'll need\nCustomer-facing program experience.\n4+ years of relevant experience in program management, implementation, customer success or consulting.\n• Excel\n• Smartsheet"
        );
    }

    #[test]
    fn html_cleanup() {
        let t = html_to_text("&lt;p&gt;Hello &amp;amp; welcome&lt;/p&gt;&lt;ul&gt;&lt;li&gt;Excel&lt;/li&gt;&lt;/ul&gt;");
        assert!(t.contains("Hello & welcome"));
        assert!(t.contains("• Excel"));
        assert_eq!(decode_entities("Caf&#233; &#x27;x&#x27;"), "Café 'x'");
    }

    #[test]
    fn pasted_posting() {
        let text = "Title: Smartsheet Administrator\nCompany: Contoso Health\nLocation: Remote (US)\n\
                    Full-time. Salary $70,000 - $85,000 per year.\nApply at https://careers.contoso.example/jobs/42?utm_source=li\n\
                    We cannot hire in Hawaii.";
        let d = parse_pasted(text);
        assert_eq!(d.title, "Smartsheet Administrator");
        assert_eq!(d.employer, "Contoso Health");
        assert_eq!(d.remote_type, "remote");
        assert_eq!(d.wisconsin_eligibility, "eligible");
        assert_eq!(d.salary_min, Some(70000.0));
        assert_eq!(d.employment_type, "Full-time");
        assert_eq!(d.apply_url.as_deref(), Some("https://careers.contoso.example/jobs/42?utm_source=li"));
    }

    #[test]
    fn pasted_without_labels_flags_guesses() {
        let d = parse_pasted("Operations Coordinator\nWe need someone great.");
        assert_eq!(d.title, "Operations Coordinator");
        assert!(d.employer.is_empty());
        assert!(d.detection_notes.iter().any(|n| n.contains("guessed")));
        assert!(d.detection_notes.iter().any(|n| n.contains("Employer not detected")));
        assert_eq!(d.remote_type, "unknown");
        assert_eq!(d.salary_min, None);
    }

    #[test]
    fn jsonld_extraction() {
        let html = r#"<html><head><script type="application/ld+json">
        {"@context":"https://schema.org","@graph":[{"@type":"Organization","name":"X"},{
          "@type":"JobPosting","title":"Data Analyst","datePosted":"2026-10-30T08:00:00Z","validThrough":"2026-12-31",
          "hiringOrganization":{"@type":"Organization","name":"Fabrikam"},
          "jobLocationType":"TELECOMMUTE",
          "applicantLocationRequirements":[{"@type":"State","name":"Illinois"},{"@type":"State","name":"Texas"}],
          "employmentType":["FULL_TIME"],
          "baseSalary":{"@type":"MonetaryAmount","currency":"USD","value":{"@type":"QuantitativeValue","minValue":"60000","maxValue":75000,"unitText":"YEAR"}},
          "description":"&lt;p&gt;Build reports in Excel.&lt;/p&gt;"}]}
        </script></head><body></body></html>"#;
        let v = find_jobposting_jsonld(html).expect("found");
        let d = draft_from_jsonld(&v, "https://jobs.example.com/1");
        assert_eq!(d.title, "Data Analyst");
        assert_eq!(d.employer, "Fabrikam");
        assert_eq!(d.remote_type, "remote");
        assert_eq!(d.wisconsin_eligibility, "excluded");
        assert_eq!(d.salary_min, Some(60000.0));
        assert_eq!(d.salary_max, Some(75000.0));
        assert_eq!(d.salary_period.as_deref(), Some("year"));
        assert_eq!(d.posted_date.as_deref(), Some("2026-10-30"));
        assert_eq!(d.employment_type, "Full-time");
        assert!(d.description.contains("Build reports in Excel."));
        assert_eq!(d.extraction_method, "json-ld");
    }

    #[test]
    fn page_titles_are_split() {
        assert_eq!(split_page_title("Acme Care Resources Inc - Manager of Intake"), ("Manager of Intake".into(), "Acme Care Resources Inc".into(), true));
        assert_eq!(split_page_title("Data Analyst | Fabrikam Careers"), ("Data Analyst".into(), "Fabrikam".into(), true));
        assert_eq!(split_page_title("Support Analyst | Litware"), ("Support Analyst".into(), "Litware".into(), false));
        assert_eq!(split_page_title("Operations Coordinator"), ("Operations Coordinator".into(), String::new(), false));
    }

    #[test]
    fn html_fallback_is_flagged() {
        let d = draft_from_html_fallback("<html><title>Support Analyst | Litware</title><body><p>Remote role</p></body></html>", "https://x.example/j");
        assert_eq!(d.title, "Support Analyst");
        assert_eq!(d.employer, "Litware");
        assert!(d.detection_notes.iter().any(|n| n.contains("were guessed")));
        assert_eq!(d.extraction_method, "heuristic");
        assert!(d.detection_notes.iter().any(|n| n.contains("No structured job data")));
    }
}
