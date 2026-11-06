//! Job-alert emails (Indeed, LinkedIn, ZipRecruiter, Glassdoor and similar). Each alert is read on
//! this Mac with rules, no AI: job links are grouped by posting, and the title, company and location
//! are taken from the text in and right after each link. Board email layouts change without notice,
//! so every job keeps a note saying it was read from an email, and anything unclear is left out
//! rather than guessed.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Senders that send job alerts, by domain suffix → display name.
pub const BOARDS: [(&str, &str); 14] = [
    ("indeed.com", "Indeed"),
    ("indeedemail.com", "Indeed"),
    ("linkedin.com", "LinkedIn"),
    ("ziprecruiter.com", "ZipRecruiter"),
    ("glassdoor.com", "Glassdoor"),
    ("monster.com", "Monster"),
    ("careerbuilder.com", "CareerBuilder"),
    ("dice.com", "Dice"),
    ("simplyhired.com", "SimplyHired"),
    ("builtin.com", "Built In"),
    ("wellfound.com", "Wellfound"),
    ("joinhandshake.com", "Handshake"),
    ("idealist.org", "Idealist"),
    ("flexjobs.com", "FlexJobs"),
];

pub fn board_for(domain: &str) -> Option<&'static str> {
    let d = domain.to_lowercase();
    BOARDS.iter().find(|(suffix, _)| d == *suffix || d.ends_with(&format!(".{suffix}"))).map(|(_, n)| *n)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AlertJob {
    pub title: String,
    pub employer: String,
    pub location: String,
    pub url: String,
    /// Extra lines from the card (salary, "Easy Apply", …), kept as the job's description.
    pub details: Vec<String>,
    /// added | duplicate | filtered | excluded | incomplete | error
    pub outcome: String,
    pub reason: String,
    pub job_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Link { href: String, lines: Vec<String> },
    Text(String),
}

static RE_DROP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<head\b.*?</head>|<style\b.*?</style>|<script\b.*?</script>|<!--.*?-->").unwrap());
static RE_ANCHOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<a\b[^>]*?\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')[^>]*>(.*?)</a\s*>"#).unwrap());
static RE_BREAK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<\s*(?:br|/?p|/?div|/?td|/?th|/?tr|/?table|/?li|/?ul|/?ol|/?h[1-6]|/?section|/?span\s+class=.?break)\b[^>]*>").unwrap()
});
static RE_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<[^>]+>").unwrap());

fn lines_of(html: &str) -> Vec<String> {
    let s = RE_BREAK.replace_all(html, "\n");
    let s = RE_TAG.replace_all(&s, " ");
    let s = crate::parse::decode_entities(&s);
    s.lines()
        .map(|l| l.replace(['\u{200c}', '\u{200b}', '\u{feff}', '\u{a0}', '\u{34f}'], " ").split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect()
}

fn pieces(html: &str) -> Vec<Piece> {
    let html = RE_DROP.replace_all(html, "");
    let mut out = vec![];
    let mut last = 0;
    for m in RE_ANCHOR.captures_iter(&html) {
        let whole = m.get(0).unwrap();
        for l in lines_of(&html[last..whole.start()]) {
            out.push(Piece::Text(l));
        }
        let href = crate::parse::decode_entities(m.get(1).or(m.get(2)).map(|x| x.as_str()).unwrap_or("")).trim().to_string();
        out.push(Piece::Link { href, lines: lines_of(m.get(3).map(|x| x.as_str()).unwrap_or("")) });
        last = whole.end();
    }
    for l in lines_of(&html[last..]) {
        out.push(Piece::Text(l));
    }
    out
}

static RE_LINKEDIN_JOB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"linkedin\.com/(?:comm/)?jobs/view/(\d{6,})").unwrap());
static RE_INDEED_JK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[?&](?:jk|vjk)=([0-9a-f]{12,20})").unwrap());
static RE_GLASSDOOR_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[?&]jobListingId=(\d+)|_JV_[^?]*?(\d{8,})").unwrap());

/// A job link's identity and its cleanest URL, when the link is recognisably a job posting.
fn job_key(href: &str) -> Option<(String, String)> {
    let h = href.to_lowercase();
    if let Some(m) = RE_LINKEDIN_JOB.captures(href) {
        return Some((format!("li:{}", &m[1]), format!("https://www.linkedin.com/jobs/view/{}/", &m[1])));
    }
    if h.contains("indeed.") {
        if let Some(m) = RE_INDEED_JK.captures(href) {
            return Some((format!("in:{}", &m[1]), format!("https://www.indeed.com/viewjob?jk={}", &m[1])));
        }
        if ["/rc/clk", "/pagead/clk", "/viewjob", "cts.indeed.com", "/job/", "engage.indeed.com"].iter().any(|p| h.contains(p)) {
            return Some((String::new(), href.to_string()));
        }
        return None;
    }
    if h.contains("glassdoor.") {
        if let Some(m) = RE_GLASSDOOR_ID.captures(href) {
            let id = m.get(1).or(m.get(2)).map(|x| x.as_str()).unwrap_or("");
            return Some((format!("gd:{id}"), href.to_string()));
        }
        return h.contains("/job-listing/").then(|| (String::new(), href.to_string()));
    }
    if h.contains("ziprecruiter.") {
        return ["/k/", "/km/", "/jobs/", "/job/", "/c/", "/ojob/", "/job-redirect"].iter().any(|p| h.contains(p)).then(|| (String::new(), href.to_string()));
    }
    // Other boards: any link that isn't plainly account or marketing furniture.
    let furniture = ["unsubscribe", "preferences", "settings", "privacy", "terms", "help", "support", "apps.apple", "play.google", "/login", "/signin", "mailto:", "/search", "/alerts", "/notifications"];
    (h.starts_with("http") && !furniture.iter().any(|f| h.contains(f))).then(|| (String::new(), href.to_string()))
}

static RE_NOT_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)^(?:
            view\b|see\b|show\b|apply\b|easy\ apply|quick\ apply|save\b|search\b|browse\b|explore\b|find\b|get\b|try\b|update\b|edit\b|manage\b|unsubscribe|
            download|sign\ in|log\ in|learn\ more|read\ more|more\b|click|tap\b|continue|upload|complete\ your|jobs?\ for\ you|recommended|
            new\b$|new\ to\ you|promoted|actively\ (?:recruiting|hiring)|be\ an\ early|urgently\ hiring|hiring\ (?:multiple|now)|just\ posted|responsive\ employer|
            top\ applicant|you\ (?:may|might|could|are|have)|your\b|we\b|this\ (?:job|email)|these\b|based\ on|similar\b|because\ you|job\ alert|daily\ digest|
            privacy|terms|help|feedback|copyright|©|indeed|linkedin|ziprecruiter|glassdoor|\$|\d+\s*(?:days?|hours?|minutes?|d|h)\b|\d+\+?\s+(?:applicants|jobs|new)|
            posted\b|reposted|today\b|yesterday|salary|estimated|employer\ est|full[-\ ]time$|part[-\ ]time$|contract$|remote$|hybrid$|on-?site$
        )",
    )
    .unwrap()
});
static RE_LOCATION: LazyLock<Regex> = LazyLock::new(|| {
    let states = crate::parse::US_STATES.join("|");
    Regex::new(&format!(
        r"(?:^|\b)(?i:remote|hybrid|on-?site|united states|usa|nationwide|anywhere)\b|,\s*(?:[A-Z]{{2}}|{states})\b|^(?:{states})$"
    ))
    .unwrap()
});
static RE_RATING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d(?:\.\d)?\s*(?:★|stars?|\*)?$").unwrap());

fn title_like(s: &str) -> bool {
    let words = s.split_whitespace().count();
    (2..=14).contains(&words) && (6..=120).contains(&s.chars().count())
        && s.chars().any(|c| c.is_alphabetic())
        && !s.ends_with('?')
        && !s.ends_with('!')
        && !RE_NOT_TITLE.is_match(s)
        || (words == 1 && s.chars().count() >= 5 && s.chars().next().is_some_and(char::is_uppercase) && !RE_NOT_TITLE.is_match(s) && !RE_LOCATION.is_match(s))
}

fn filler(s: &str) -> bool {
    RE_RATING.is_match(s) || RE_NOT_TITLE.is_match(s) || s.chars().count() < 2 || s.chars().all(|c| !c.is_alphanumeric())
}

/// Splits "Acme Care · Milwaukee, WI (Remote)" into company and location.
fn split_company_location(s: &str) -> (String, Option<String>) {
    for sep in [" · ", " • ", " | ", " – ", " — ", " - "] {
        if let Some((a, b)) = s.split_once(sep) {
            if RE_LOCATION.is_match(b) && !a.trim().is_empty() {
                return (a.trim().to_string(), Some(b.trim().to_string()));
            }
        }
    }
    (s.trim().to_string(), None)
}

/// The jobs an alert email lists, in the order they appear. Not yet saved; `outcome` is empty.
pub fn parse(html: &str) -> Vec<AlertJob> {
    let ps = pieces(html);
    // Group lines by posting: the link's own lines plus the plain text right after it.
    let mut groups: Vec<(String, String, Vec<String>)> = vec![]; // (key, url, lines)
    let mut current: Option<usize> = None;
    for p in &ps {
        match p {
            Piece::Link { href, lines } => {
                current = None;
                let Some((key, url)) = job_key(href) else { continue };
                if lines.is_empty() {
                    continue; // image link: its text comes from the title link of the same job
                }
                let idx = if key.is_empty() {
                    // No posting id: a new group unless this link only repeats the previous one.
                    match groups.last() {
                        Some((k, u, _)) if k.is_empty() && *u == url => groups.len() - 1,
                        _ => {
                            if !lines.iter().any(|l| title_like(l)) && groups.last().is_some() {
                                // "View job" / company link for the posting above.
                                groups.len() - 1
                            } else {
                                groups.push((String::new(), url.clone(), vec![]));
                                groups.len() - 1
                            }
                        }
                    }
                } else if let Some(i) = groups.iter().position(|(k, _, _)| *k == key) {
                    i
                } else {
                    groups.push((key, url.clone(), vec![]));
                    groups.len() - 1
                };
                for l in lines {
                    if !groups[idx].2.contains(l) {
                        groups[idx].2.push(l.clone());
                    }
                }
                current = Some(idx);
            }
            Piece::Text(t) => {
                if let Some(i) = current {
                    if groups[i].2.len() < 12 && !groups[i].2.contains(t) {
                        groups[i].2.push(t.clone());
                    }
                }
            }
        }
    }
    let mut out: Vec<AlertJob> = vec![];
    for (_, url, lines) in groups {
        let Some(ti) = lines.iter().position(|l| title_like(l)) else { continue };
        let title = lines[ti].clone();
        let rest: Vec<&String> = lines[ti + 1..].iter().filter(|l| !filler(l) || RE_LOCATION.is_match(l)).collect();
        let mut employer = String::new();
        let mut location = String::new();
        let mut used = 0;
        if let Some(first) = rest.first() {
            let (co, loc) = split_company_location(first);
            if !RE_LOCATION.is_match(&co) || co.contains('·') {
                employer = co;
                used = 1;
                if let Some(l) = loc {
                    location = l;
                }
            }
        }
        if location.is_empty() {
            if let Some(l) = rest.get(used).filter(|l| RE_LOCATION.is_match(l) && l.chars().count() <= 80) {
                location = l.to_string();
                used += 1;
            }
        }
        let details: Vec<String> = lines[ti + 1..]
            .iter()
            .filter(|l| **l != employer && **l != location && !rest.iter().take(used).any(|r| r == l))
            .filter(|l| l.chars().count() <= 160)
            .take(6)
            .cloned()
            .collect();
        if employer.chars().count() > 80 || employer.split_whitespace().count() > 8 {
            employer.clear();
        }
        let dup = out.iter().any(|j| j.title.eq_ignore_ascii_case(&title) && j.employer.eq_ignore_ascii_case(&employer));
        if !dup {
            out.push(AlertJob { title, employer, location, url, details, ..Default::default() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINKEDIN: &str = r#"<html><head><style>.x{}</style></head><body><table>
      <tr><td><a href="https://www.linkedin.com/comm/jobs/view/4012345678/?trackingId=abc&amp;refId=1"><img src="logo.png"></a></td>
          <td><a href="https://www.linkedin.com/comm/jobs/view/4012345678/?trackingId=abc">Intake Operations Manager</a>
              <p>Acme Care · Milwaukee, WI (Remote)</p><p>Actively recruiting</p><p>$70K-$80K / year</p></td></tr>
      <tr><td><a href="https://www.linkedin.com/comm/jobs/view/4099999999/?x=1">Senior Recruiting Coordinator</a>
              <p>Northwind Health · United States (Remote)</p><p>Easy Apply</p></td></tr>
      <tr><td><a href="https://www.linkedin.com/comm/jobs/search/?keywords=intake">See all jobs</a></td></tr>
      <tr><td><a href="https://www.linkedin.com/comm/psettings/email-unsubscribe?x">Unsubscribe</a></td></tr>
    </table></body></html>"#;

    const INDEED: &str = r#"<table>
      <tr><td><a href="https://cts.indeed.com/v3/H4sIAAAAAAAA_abc"><b>Call Center Supervisor</b></a></td></tr>
      <tr><td>Contoso Behavioral</td></tr><tr><td>Madison, WI</td></tr><tr><td>$55,000 - $65,000 a year</td></tr>
      <tr><td>Easily apply</td></tr><tr><td><a href="https://cts.indeed.com/v3/H4sIAAAAAAAA_abc">View job</a></td></tr>
      <tr><td><a href="https://www.indeed.com/rc/clk/dl?jk=0123456789abcdef&amp;from=ja">Patient Access Manager</a></td></tr>
      <tr><td>Fabrikam Clinics - Remote</td></tr><tr><td>Just posted</td></tr>
      <tr><td><a href="https://www.indeed.com/jobs?q=intake">See more jobs</a></td></tr>
      <tr><td><a href="https://www.indeed.com/unsubscribe?x">Unsubscribe</a></td></tr>
    </table>"#;

    #[test]
    fn reads_a_linkedin_alert() {
        let jobs = parse(LINKEDIN);
        assert_eq!(jobs.len(), 2, "{jobs:#?}");
        assert_eq!(jobs[0].title, "Intake Operations Manager");
        assert_eq!(jobs[0].employer, "Acme Care");
        assert_eq!(jobs[0].location, "Milwaukee, WI (Remote)");
        assert_eq!(jobs[0].url, "https://www.linkedin.com/jobs/view/4012345678/");
        assert!(jobs[0].details.iter().any(|d| d.contains("$70K")));
        assert_eq!(jobs[1].employer, "Northwind Health");
    }

    #[test]
    fn reads_an_indeed_alert() {
        let jobs = parse(INDEED);
        assert_eq!(jobs.len(), 2, "{jobs:#?}");
        assert_eq!(jobs[0].title, "Call Center Supervisor");
        assert_eq!(jobs[0].employer, "Contoso Behavioral");
        assert_eq!(jobs[0].location, "Madison, WI");
        assert!(jobs[0].details.iter().any(|d| d.contains("$55,000")));
        assert_eq!(jobs[1].title, "Patient Access Manager");
        assert_eq!(jobs[1].employer, "Fabrikam Clinics");
        assert_eq!(jobs[1].location, "Remote");
        assert_eq!(jobs[1].url, "https://www.indeed.com/viewjob?jk=0123456789abcdef");
    }

    #[test]
    fn ignores_newsletters_without_job_cards() {
        let html = r#"<p>Tips for your search</p><a href="https://www.indeed.com/career-advice/x">Read more</a><a href="https://www.indeed.com/settings">Settings</a>"#;
        assert!(parse(html).is_empty());
    }

    #[test]
    fn knows_the_boards() {
        assert_eq!(board_for("jobalerts-noreply.linkedin.com"), Some("LinkedIn"));
        assert_eq!(board_for("indeed.com"), Some("Indeed"));
        assert_eq!(board_for("notindeed.com"), None);
        assert_eq!(board_for("acmecare.example"), None);
    }

    #[test]
    fn title_filter() {
        assert!(title_like("Intake Operations Manager"));
        assert!(!title_like("View job"));
        assert!(!title_like("See all jobs"));
        assert!(!title_like("3 days ago"));
        assert!(!title_like("$55,000 - $65,000 a year"));
        assert!(!title_like("Unsubscribe"));
    }
}
