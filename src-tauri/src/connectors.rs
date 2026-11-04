//! Job-source connectors. Each uses a documented, public, unauthenticated endpoint that the
//! provider publishes for exactly this purpose (embedding/listing job postings):
//!
//! | kind       | endpoint                                                               |
//! |------------|------------------------------------------------------------------------|
//! | greenhouse | `https://boards-api.greenhouse.io/v1/boards/{token}/jobs?content=true` |
//! | lever      | `https://api.lever.co/v0/postings/{site}?mode=json`                    |
//! | ashby      | `https://api.ashbyhq.com/posting-api/job-board/{name}?includeCompensation=true` |
//! | remotive   | `https://remotive.com/api/remote-jobs?limit=100` (its search filter is currently ignored) |
//! | himalayas  | `https://himalayas.app/jobs/api/search?q={query}&country=US&limit=20`  |
//! | jobicy     | `https://jobicy.com/api/v2/remote-jobs?count=50&geo=usa&tag={query}` (credit Jobicy; apply via original URL) |
//!
//! No logins, no scraping of rendered pages, no CAPTCHA or bot-protection bypass.

use crate::models::{JobDraft, JobSource};
use crate::parse;
use serde_json::Value;

pub const KINDS: [&str; 6] = ["greenhouse", "lever", "ashby", "remotive", "himalayas", "jobicy"];
/// Boards searched by keyword rather than by company.
pub const KEYWORD_KINDS: [&str; 3] = ["remotive", "himalayas", "jobicy"];

fn s(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).map(|x| x.trim().to_string()).unwrap_or_default()
}
fn f(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(x) => x.replace([',', '$'], "").parse().ok(),
        _ => None,
    }
}
fn date(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(x) => parse::normalize_date(x),
        Value::Number(n) => {
            let t = n.as_i64()?;
            // Seconds (Himalayas) or milliseconds (Lever)
            let ms = if t < 100_000_000_000 { t * 1000 } else { t };
            chrono::DateTime::from_timestamp_millis(ms).map(|d| d.date_naive().to_string())
        }
        _ => None,
    }
}
fn http(u: String) -> Option<String> {
    u.starts_with("http").then_some(u)
}

fn base(source: &str) -> JobDraft {
    JobDraft { source: source.into(), extraction_method: "api".into(), ..Default::default() }
}

/// Display name for a board when the API does not return the company name.
pub fn company_label(src: &JobSource) -> String {
    if !src.label.trim().is_empty() {
        return src.label.trim().to_string();
    }
    let id = src.identifier.replace(['-', '_'], " ");
    id.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_greenhouse(v: &Value, src: &JobSource) -> Vec<JobDraft> {
    let fallback = company_label(src);
    v.get("jobs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|j| {
            let mut d = base(&format!("greenhouse:{}", src.identifier));
            d.title = s(j.get("title"));
            d.employer = Some(s(j.get("company_name"))).filter(|x| !x.is_empty()).unwrap_or_else(|| fallback.clone());
            d.location_text = s(j.get("location").and_then(|l| l.get("name")));
            d.description = parse::html_to_text(&s(j.get("content")));
            d.posted_date = date(j.get("first_published")).or_else(|| date(j.get("updated_at")));
            d.valid_through = date(j.get("application_deadline"));
            d.posting_url = http(s(j.get("absolute_url")));
            d.apply_url = d.posting_url.clone();
            d.detection_notes.push(format!("From {}'s public Greenhouse job board", d.employer));
            parse::enrich(&mut d);
            d
        })
        .filter(|d| !d.title.is_empty())
        .collect()
}

pub fn parse_lever(v: &Value, src: &JobSource) -> Vec<JobDraft> {
    let employer = company_label(src);
    v.as_array()
        .into_iter()
        .flatten()
        .map(|j| {
            let mut d = base(&format!("lever:{}", src.identifier));
            d.title = s(j.get("text"));
            d.employer = employer.clone();
            let cat = j.get("categories");
            d.location_text = s(cat.and_then(|c| c.get("location")));
            let mut desc = vec![s(j.get("descriptionPlain"))];
            for l in j.get("lists").and_then(Value::as_array).into_iter().flatten() {
                desc.push(format!("{}\n{}", s(l.get("text")), parse::html_to_text(&s(l.get("content")))));
            }
            desc.push(s(j.get("additionalPlain")));
            desc.push(s(j.get("salaryDescriptionPlain")));
            d.description = desc.into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("\n\n");
            d.remote_type = match s(j.get("workplaceType")).as_str() {
                "remote" => "remote",
                "hybrid" => "hybrid",
                "onsite" | "on-site" => "onsite",
                _ => "unknown",
            }
            .into();
            if d.remote_type != "unknown" {
                d.detection_notes.push(format!("Workplace type from Lever: {}", d.remote_type));
            }
            if let Some(sr) = j.get("salaryRange") {
                d.salary_min = f(sr.get("min"));
                d.salary_max = f(sr.get("max"));
                let interval = s(sr.get("interval"));
                d.salary_period = Some(if interval.contains("hour") { "hour" } else if interval.contains("month") { "month" } else { "year" }.into());
                d.detection_notes.push("Pay from Lever's structured salary range".into());
            }
            d.employment_type = parse::detect_employment_type(&s(cat.and_then(|c| c.get("commitment"))));
            d.posted_date = date(j.get("createdAt"));
            d.posting_url = http(s(j.get("hostedUrl")));
            d.apply_url = http(s(j.get("applyUrl"))).or_else(|| d.posting_url.clone());
            d.detection_notes.push(format!("From {employer}'s public Lever job board"));
            parse::enrich(&mut d);
            d
        })
        .filter(|d| !d.title.is_empty())
        .collect()
}

pub fn parse_ashby(v: &Value, src: &JobSource) -> Vec<JobDraft> {
    let employer = company_label(src);
    v.get("jobs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|j| j.get("isListed").and_then(Value::as_bool).unwrap_or(true))
        .map(|j| {
            let mut d = base(&format!("ashby:{}", src.identifier));
            d.title = s(j.get("title"));
            d.employer = employer.clone();
            let mut locs = vec![s(j.get("location"))];
            for l in j.get("secondaryLocations").and_then(Value::as_array).into_iter().flatten() {
                locs.push(s(l.get("location")));
            }
            let country = s(j.get("address").and_then(|a| a.get("postalAddress")).and_then(|p| p.get("addressCountry")));
            if !country.is_empty() {
                locs.push(country);
            }
            d.location_text = locs.into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
            d.description = Some(s(j.get("descriptionPlain")))
                .filter(|x| !x.is_empty())
                .unwrap_or_else(|| parse::html_to_text(&s(j.get("descriptionHtml"))));
            let wt = s(j.get("workplaceType")).to_lowercase();
            d.remote_type = if wt.contains("hybrid") {
                "hybrid"
            } else if wt.contains("onsite") || wt.contains("on-site") {
                "onsite"
            } else if wt.contains("remote") || j.get("isRemote").and_then(Value::as_bool) == Some(true) {
                "remote"
            } else {
                "unknown"
            }
            .into();
            if d.remote_type != "unknown" {
                d.detection_notes.push(format!("Workplace type from Ashby: {}", d.remote_type));
            }
            d.employment_type = match s(j.get("employmentType")).as_str() {
                "FullTime" => "Full-time",
                "PartTime" => "Part-time",
                "Contract" => "Contract",
                "Temporary" => "Temporary",
                "Intern" => "Internship",
                _ => "",
            }
            .into();
            let comp = j.get("compensation");
            let mut comps: Vec<&Value> = comp.and_then(|c| c.get("summaryComponents")).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
            for t in comp.and_then(|c| c.get("compensationTiers")).and_then(Value::as_array).into_iter().flatten() {
                comps.extend(t.get("components").and_then(Value::as_array).into_iter().flatten());
            }
            if let Some(sal) = comps.into_iter().find(|c| s(c.get("compensationType")) == "Salary" && f(c.get("minValue")).is_some()) {
                d.salary_min = f(sal.get("minValue"));
                d.salary_max = f(sal.get("maxValue")).or(d.salary_min);
                let iv = s(sal.get("interval")).to_uppercase();
                d.salary_period = Some(if iv.contains("HOUR") { "hour" } else if iv.contains("MONTH") { "month" } else { "year" }.into());
                d.detection_notes.push("Pay from Ashby's structured compensation".into());
            }
            d.posted_date = date(j.get("publishedAt"));
            d.posting_url = http(s(j.get("jobUrl")));
            d.apply_url = http(s(j.get("applyUrl"))).or_else(|| d.posting_url.clone());
            d.detection_notes.push(format!("From {employer}'s public Ashby job board"));
            parse::enrich(&mut d);
            d
        })
        .filter(|d| !d.title.is_empty())
        .collect()
}

/// Remotive's `candidate_required_location` → Wisconsin eligibility.
pub fn remotive_location(loc: &str) -> (&'static str, String) {
    let l = loc.to_lowercase();
    if l.trim().is_empty() {
        return ("unknown", "Remotive lists no location requirement".into());
    }
    let ok = ["usa", "us ", "u.s", "united states", "worldwide", "anywhere", "americas", "north america"];
    if ok.iter().any(|k| l.contains(k)) || l.trim() == "us" || l.contains("wisconsin") {
        ("eligible", format!("Remotive location requirement: {loc}"))
    } else {
        ("excluded", format!("Remotive location requirement ({loc}) does not include the US"))
    }
}

pub fn parse_remotive(v: &Value) -> Vec<JobDraft> {
    v.get("jobs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|j| {
            let mut d = base("remotive");
            d.title = s(j.get("title"));
            d.employer = s(j.get("company_name"));
            let loc = s(j.get("candidate_required_location"));
            d.location_text = if loc.is_empty() { "Remote".into() } else { format!("Remote — {loc}") };
            d.remote_type = "remote".into();
            d.detection_notes.push("Listed on Remotive, a remote-only job board".into());
            let (w, note) = remotive_location(&loc);
            d.wisconsin_eligibility = w.into();
            d.detection_notes.push(note);
            d.description = parse::html_to_text(&s(j.get("description")));
            let salary = s(j.get("salary"));
            if let Some((lo, hi, p)) = parse::parse_salary(&salary) {
                d.salary_min = Some(lo);
                d.salary_max = Some(hi);
                d.salary_period = Some(p);
                d.detection_notes.push(format!("Pay from Remotive's salary field “{salary}”"));
            }
            d.employment_type = match s(j.get("job_type")).as_str() {
                "full_time" => "Full-time",
                "part_time" => "Part-time",
                "contract" | "freelance" => "Contract",
                "internship" => "Internship",
                _ => "",
            }
            .into();
            d.posted_date = date(j.get("publication_date"));
            d.posting_url = http(s(j.get("url")));
            d.apply_url = d.posting_url.clone();
            parse::enrich(&mut d);
            d
        })
        .filter(|d| !d.title.is_empty() && !d.employer.is_empty())
        .collect()
}

fn period(p: &str) -> String {
    let p = p.to_lowercase();
    if p.contains("hour") { "hour" } else if p.contains("month") { "month" } else if p.contains("week") { "week" } else { "year" }.into()
}

fn employment(t: &str) -> String {
    let t = t.to_lowercase();
    if t.contains("part") {
        "Part-time"
    } else if t.contains("contract") || t.contains("freelance") {
        "Contract"
    } else if t.contains("temp") {
        "Temporary"
    } else if t.contains("intern") {
        "Internship"
    } else if t.contains("full") {
        "Full-time"
    } else {
        ""
    }
    .into()
}

pub fn parse_himalayas(v: &Value) -> Vec<JobDraft> {
    v.get("jobs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|j| {
            let mut d = base("himalayas");
            d.title = s(j.get("title"));
            d.employer = s(j.get("companyName"));
            let locs: Vec<String> = j.get("locationRestrictions").and_then(Value::as_array).into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
            d.location_text = if locs.is_empty() { "Remote — anywhere".into() } else { format!("Remote — {}", locs.join(", ")) };
            d.remote_type = "remote".into();
            d.detection_notes.push("Listed on Himalayas, a remote-only job board".into());
            let (w, note) = if locs.is_empty() { ("eligible", "Himalayas lists no location restriction".to_string()) } else { remotive_location(&locs.join(", ")) };
            d.wisconsin_eligibility = w.into();
            d.detection_notes.push(note.replace("Remotive", "Himalayas"));
            d.description = parse::html_to_text(&s(j.get("description")));
            d.salary_min = f(j.get("minSalary"));
            d.salary_max = f(j.get("maxSalary")).or(d.salary_min);
            if d.salary_min.is_some() {
                d.salary_period = Some(period(&s(j.get("salaryPeriod"))));
                d.detection_notes.push("Pay from Himalayas' salary fields".into());
            }
            d.employment_type = employment(&s(j.get("employmentType")));
            d.posted_date = date(j.get("pubDate"));
            d.valid_through = date(j.get("expiryDate"));
            d.posting_url = http(s(j.get("guid")));
            d.apply_url = http(s(j.get("applicationLink"))).or_else(|| d.posting_url.clone());
            parse::enrich(&mut d);
            d
        })
        .filter(|d| !d.title.is_empty() && !d.employer.is_empty())
        .collect()
}

pub fn parse_jobicy(v: &Value) -> Vec<JobDraft> {
    v.get("jobs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|j| {
            let mut d = base("jobicy");
            d.title = parse::decode_entities(&s(j.get("jobTitle")));
            d.employer = parse::decode_entities(&s(j.get("companyName")));
            let geo = s(j.get("jobGeo"));
            d.location_text = format!("Remote — {}", if geo.is_empty() { "anywhere" } else { &geo });
            d.remote_type = "remote".into();
            d.detection_notes.push("Listed on Jobicy, a remote-only job board (via Jobicy)".into());
            let (w, note) = remotive_location(if geo.is_empty() { "anywhere" } else { &geo });
            d.wisconsin_eligibility = w.into();
            d.detection_notes.push(note.replace("Remotive", "Jobicy"));
            d.description = parse::html_to_text(&s(j.get("jobDescription")));
            d.salary_min = f(j.get("salaryMin"));
            d.salary_max = f(j.get("salaryMax")).or(d.salary_min);
            if d.salary_min.is_some() {
                d.salary_period = Some(period(&s(j.get("salaryPeriod"))));
                d.detection_notes.push("Pay from Jobicy's salary fields".into());
            }
            let types: Vec<String> = match j.get("jobType") {
                Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str()).map(employment).filter(|x| !x.is_empty()).collect(),
                Some(Value::String(x)) => vec![employment(x)],
                _ => vec![],
            };
            d.employment_type = types.join(", ");
            d.posted_date = date(j.get("pubDate"));
            d.posting_url = http(s(j.get("url")));
            d.apply_url = d.posting_url.clone();
            parse::enrich(&mut d);
            d
        })
        .filter(|d| !d.title.is_empty() && !d.employer.is_empty())
        .collect()
}

/// Recognise a careers-page URL and return the matching connector.
pub fn detect_source(url: &str) -> Option<(String, String)> {
    let u = url::Url::parse(url.trim()).ok()?;
    let host = u.host_str()?.to_lowercase();
    let seg: Vec<&str> = u.path_segments().map(|s| s.filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    let first = seg.first().map(|s| s.to_string());
    let q = |k: &str| u.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.to_string());
    if host.ends_with("greenhouse.io") {
        if host.starts_with("boards-api") {
            let i = seg.iter().position(|x| *x == "boards")?;
            return Some(("greenhouse".into(), seg.get(i + 1)?.to_string()));
        }
        return q("for").or(first).map(|t| ("greenhouse".into(), t));
    }
    if host == "jobs.lever.co" || host == "api.lever.co" {
        return if host == "api.lever.co" { seg.get(2).map(|t| ("lever".into(), t.to_string())) } else { first.map(|t| ("lever".into(), t)) };
    }
    if host == "jobs.ashbyhq.com" {
        return first.map(|t| ("ashby".into(), t));
    }
    if host == "api.ashbyhq.com" {
        return seg.last().map(|t| ("ashby".into(), t.to_string()));
    }
    None
}

pub fn endpoint(src: &JobSource, query: Option<&str>) -> Result<String, String> {
    let id = src.identifier.trim();
    let enc = |x: &str| url::form_urlencoded::byte_serialize(x.as_bytes()).collect::<String>();
    match src.kind.as_str() {
        "greenhouse" => Ok(format!("https://boards-api.greenhouse.io/v1/boards/{}/jobs?content=true", enc(id))),
        "lever" => Ok(format!("https://api.lever.co/v0/postings/{}?mode=json", enc(id))),
        "ashby" => Ok(format!("https://api.ashbyhq.com/posting-api/job-board/{}?includeCompensation=true", enc(id))),
        "remotive" => Ok("https://remotive.com/api/remote-jobs?limit=100".to_string()),
        "himalayas" => Ok(format!("https://himalayas.app/jobs/api/search?q={}&country=US&limit=20", enc(query.unwrap_or("")))),
        "jobicy" => Ok(format!("https://jobicy.com/api/v2/remote-jobs?count=50&geo=usa&tag={}", enc(query.unwrap_or("")))),
        k => Err(format!("Unknown source type “{k}”")),
    }
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("ProjectNextChapter/", env!("CARGO_PKG_VERSION"), " (personal job-search tracker)"))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("http client")
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    let resp = client.get(url).send().await.map_err(|e| format!("Network error: {e}"))?;
    let st = resp.status();
    if st.as_u16() == 404 {
        return Err("Board not found (HTTP 404) — check the company's board name".into());
    }
    if st.as_u16() == 429 {
        return Err("The source asked us to slow down (HTTP 429); it will be retried at the next scheduled run".into());
    }
    if !st.is_success() {
        return Err(format!("Source returned HTTP {st}"));
    }
    resp.json::<Value>().await.map_err(|e| format!("Unexpected response format: {e}"))
}

/// Fetches and parses one source. Remotive runs one request per search query, spaced out.
pub async fn fetch(client: &reqwest::Client, src: &JobSource, queries: &[String]) -> Result<Vec<JobDraft>, String> {
    match src.kind.as_str() {
        "greenhouse" => Ok(parse_greenhouse(&get_json(client, &endpoint(src, None)?).await?, src)),
        "lever" => Ok(parse_lever(&get_json(client, &endpoint(src, None)?).await?, src)),
        "ashby" => Ok(parse_ashby(&get_json(client, &endpoint(src, None)?).await?, src)),
        "remotive" => Ok(parse_remotive(&get_json(client, &endpoint(src, None)?).await?)),
        "himalayas" | "jobicy" => {
            let mut out: Vec<JobDraft> = vec![];
            let mut errors = vec![];
            for (i, q) in queries.iter().filter(|q| !q.trim().is_empty()).take(10).enumerate() {
                if i > 0 {
                    tokio_sleep(2000).await;
                }
                match get_json(client, &endpoint(src, Some(q))?).await {
                    Ok(v) => {
                        let parsed = if src.kind == "himalayas" { parse_himalayas(&v) } else { parse_jobicy(&v) };
                        for d in parsed {
                            if !out.iter().any(|o| o.posting_url == d.posting_url) {
                                out.push(d);
                            }
                        }
                    }
                    Err(e) => errors.push(format!("“{q}”: {e}")),
                }
            }
            if out.is_empty() && !errors.is_empty() {
                Err(errors.join("; "))
            } else {
                Ok(out)
            }
        }
        k => Err(format!("Unknown source type “{k}”")),
    }
}

async fn tokio_sleep(ms: u64) {
    tauri::async_runtime::spawn_blocking(move || std::thread::sleep(std::time::Duration::from_millis(ms))).await.ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn src(kind: &str, id: &str, label: &str) -> JobSource {
        JobSource { kind: kind.into(), identifier: id.into(), label: label.into(), enabled: true, ..Default::default() }
    }

    #[test]
    fn greenhouse() {
        let v = json!({"jobs": [{
            "title": "Smartsheet Administrator", "company_name": "Acme", "location": {"name": "Remote - US"},
            "content": "&lt;p&gt;Fully remote. Salary $70,000 - $85,000 per year.&lt;/p&gt;&lt;h3&gt;Requirements&lt;/h3&gt;&lt;ul&gt;&lt;li&gt;Smartsheet&lt;/li&gt;&lt;/ul&gt;",
            "first_published": "2026-10-30T10:00:00-04:00", "absolute_url": "https://job-boards.greenhouse.io/acme/jobs/1"
        }]});
        let d = &parse_greenhouse(&v, &src("greenhouse", "acme", ""))[0];
        assert_eq!(d.employer, "Acme");
        assert_eq!(d.remote_type, "remote");
        assert_eq!(d.salary_min, Some(70000.0));
        assert_eq!(d.posted_date.as_deref(), Some("2026-10-30"));
        assert!(d.description.contains("• Smartsheet"));
        assert_eq!(d.source, "greenhouse:acme");
    }

    #[test]
    fn lever() {
        let v = json!([{
            "text": "Operations Coordinator", "categories": {"location": "Remote", "commitment": "Full-time"},
            "workplaceType": "remote", "createdAt": 1793000000000i64, "hostedUrl": "https://jobs.lever.co/acme/1",
            "applyUrl": "https://jobs.lever.co/acme/1/apply", "descriptionPlain": "Join us.",
            "lists": [{"text": "Requirements", "content": "<li>Excel</li>"}],
            "salaryRange": {"min": 25, "max": 30, "currency": "USD", "interval": "per-hour-wage"}
        }]);
        let d = &parse_lever(&v, &src("lever", "acme-co", ""))[0];
        assert_eq!(d.employer, "Acme Co");
        assert_eq!(d.salary_period.as_deref(), Some("hour"));
        assert_eq!(d.employment_type, "Full-time");
        assert!(d.description.contains("• Excel"));
        assert_eq!(d.apply_url.as_deref(), Some("https://jobs.lever.co/acme/1/apply"));
        assert!(d.posted_date.is_some());
    }

    #[test]
    fn ashby() {
        let v = json!({"jobs": [
            {"title": "Support Lead", "location": "Remote", "isRemote": true, "workplaceType": "Remote", "employmentType": "FullTime",
             "descriptionPlain": "Help customers.", "publishedAt": "2026-10-12T20:55:28.312+00:00", "isListed": true,
             "jobUrl": "https://jobs.ashbyhq.com/acme/x", "applyUrl": "https://jobs.ashbyhq.com/acme/x/application",
             "address": {"postalAddress": {"addressCountry": "USA"}},
             "compensation": {"summaryComponents": [], "compensationTiers": [{"components": [
                {"compensationType": "EquityPercentage", "interval": "NONE", "minValue": null},
                {"compensationType": "Salary", "interval": "1 YEAR", "minValue": 90000, "maxValue": 110000}]}]}},
            {"title": "Hidden", "isListed": false}
        ]});
        let r = parse_ashby(&v, &src("ashby", "acme", "Acme Inc"));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].employer, "Acme Inc");
        assert_eq!(r[0].salary_max, Some(110000.0));
        assert_eq!(r[0].remote_type, "remote");
        assert_eq!(r[0].location_text, "Remote · USA");
    }

    #[test]
    fn remotive() {
        let v = json!({"jobs": [
            {"title": "Data Analyst", "company_name": "Beta", "candidate_required_location": "USA Only", "salary": "$60k - $80k",
             "job_type": "full_time", "publication_date": "2026-11-01T12:00:00", "url": "https://remotive.com/remote-jobs/1", "description": "<p>SQL</p>"},
            {"title": "Copywriter", "company_name": "Gamma", "candidate_required_location": "Europe", "url": "https://remotive.com/remote-jobs/2"}
        ]});
        let r = parse_remotive(&v);
        assert_eq!(r[0].wisconsin_eligibility, "eligible");
        assert_eq!(r[0].salary_min, Some(60000.0));
        assert_eq!(r[0].employment_type, "Full-time");
        assert_eq!(r[1].wisconsin_eligibility, "excluded");
    }

    #[test]
    fn himalayas_and_jobicy() {
        let h = json!({"jobs": [
            {"title": "Operations Coordinator", "companyName": "Pavago", "locationRestrictions": ["United States"], "pubDate": 1793293572,
             "expiryDate": 1798477572, "minSalary": 55000, "maxSalary": 65000, "salaryPeriod": "annual", "employmentType": "Full Time",
             "guid": "https://himalayas.app/companies/pavago/jobs/1", "applicationLink": "https://himalayas.app/companies/pavago/jobs/1?utm=x", "description": "<p>Excel</p>"},
            {"title": "Support Agent", "companyName": "Foo", "locationRestrictions": ["Philippines"], "guid": "https://himalayas.app/x/2"}
        ]});
        let r = parse_himalayas(&h);
        assert_eq!(r[0].wisconsin_eligibility, "eligible");
        assert_eq!(r[0].salary_min, Some(55000.0));
        assert_eq!(r[0].employment_type, "Full-time");
        assert!(r[0].posted_date.as_deref().unwrap().starts_with("2026-"));
        assert!(r[0].valid_through.is_some());
        assert_eq!(r[1].wisconsin_eligibility, "excluded");

        let j = json!({"jobs": [{"jobTitle": "People &amp; Ops Partner", "companyName": "Customer.io", "jobGeo": "LATAM,  Canada,  USA",
            "pubDate": "2026-11-04T13:52:23+00:00", "salaryMin": 100000, "salaryMax": 130000, "salaryPeriod": "yearly",
            "jobType": ["Full-Time"], "url": "https://jobicy.com/jobs/158965", "jobDescription": "<p>HRIS</p>"}]});
        let r = parse_jobicy(&j);
        assert_eq!(r[0].title, "People & Ops Partner");
        assert_eq!(r[0].wisconsin_eligibility, "eligible");
        assert_eq!(r[0].apply_url.as_deref(), Some("https://jobicy.com/jobs/158965"), "apply via the original Jobicy URL");
        assert_eq!(r[0].employment_type, "Full-time");
        assert!(r[0].detection_notes.iter().any(|n| n.contains("via Jobicy")));
    }

    #[test]
    fn detect_boards() {
        assert_eq!(detect_source("https://job-boards.greenhouse.io/smartsheet/jobs/8247017"), Some(("greenhouse".into(), "smartsheet".into())));
        assert_eq!(detect_source("https://boards.greenhouse.io/embed/job_board?for=acme"), Some(("greenhouse".into(), "acme".into())));
        assert_eq!(detect_source("https://jobs.lever.co/civitech/733d4a59"), Some(("lever".into(), "civitech".into())));
        assert_eq!(detect_source("https://jobs.ashbyhq.com/vetcove/4c78f852"), Some(("ashby".into(), "vetcove".into())));
        assert_eq!(detect_source("https://www.example.com/careers"), None);
        assert!(endpoint(&src("bogus", "x", ""), None).is_err());
        assert_eq!(endpoint(&src("jobicy", "", ""), Some("data analyst")).unwrap(), "https://jobicy.com/api/v2/remote-jobs?count=50&geo=usa&tag=data+analyst");
    }
}
