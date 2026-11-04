//! Mandatory-requirement checks against the applicant's preferences.
//!
//! `excluded_reasons` = the posting clearly violates a mandatory requirement.
//! `review_flags`     = information is missing or ambiguous; a human must look.
//! Nothing here assumes a job qualifies when the posting is silent.

use crate::models::{JobDraft, Preferences};
use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// meets | partial | below | missing
    pub salary_flag: String,
    pub review_flags: Vec<String>,
    pub excluded_reasons: Vec<String>,
}

pub fn annualize(amount: f64, period: &str, hours_per_year: f64) -> f64 {
    match period {
        "hour" => amount * hours_per_year,
        "day" => amount * (hours_per_year / 8.0),
        "week" => amount * 52.0,
        "month" => amount * 12.0,
        _ => amount,
    }
}

pub fn money(v: f64) -> String {
    let n = v.round() as i64;
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{}${}", if n < 0 { "-" } else { "" }, out)
}

pub fn is_expired(valid_through: Option<&str>, today: NaiveDate) -> bool {
    valid_through
        .and_then(|v| NaiveDate::parse_from_str(v.get(..10).unwrap_or(v), "%Y-%m-%d").ok())
        .map(|d| d < today)
        .unwrap_or(false)
}

pub fn evaluate(d: &JobDraft, p: &Preferences, today: NaiveDate) -> Evaluation {
    let mut review = Vec::new();
    let mut excluded = Vec::new();
    let period = d.salary_period.clone().unwrap_or_else(|| "year".into());

    // Compensation
    let salary_flag = match (d.salary_min, d.salary_max) {
        (None, None) => {
            review.push("Salary not listed — confirm compensation before investing time".to_string());
            "missing"
        }
        (lo, hi) => {
            let lo_a = annualize(lo.or(hi).unwrap_or(0.0), &period, p.hours_per_year);
            let hi_a = annualize(hi.or(lo).unwrap_or(0.0), &period, p.hours_per_year);
            let eq = if period == "year" { String::new() } else { format!(" (annualised at {} h/yr)", p.hours_per_year) };
            if hi_a < p.min_annual_salary {
                excluded.push(format!(
                    "Top of listed pay {}{} is below your minimum of {}",
                    money(hi_a),
                    eq,
                    money(p.min_annual_salary)
                ));
                "below"
            } else if lo_a >= p.min_annual_salary {
                "meets"
            } else {
                review.push(format!(
                    "Pay range {}–{}{} straddles your minimum of {}",
                    money(lo_a),
                    money(hi_a),
                    eq,
                    money(p.min_annual_salary)
                ));
                "partial"
            }
        }
    };

    // Work arrangement
    if p.remote_only {
        match d.remote_type.as_str() {
            "onsite" => excluded.push("Requires on-site work; you require 100% remote".into()),
            "hybrid" => excluded.push("Hybrid role with office attendance; you require 100% remote".into()),
            "remote" => {}
            _ => review.push("Work arrangement not stated — confirm the role is fully remote".into()),
        }
    }

    // Geographic eligibility
    match d.wisconsin_eligibility.as_str() {
        "excluded" => excluded.push(format!("Employer does not appear to hire in {}", p.home_state)),
        "eligible" => {}
        _ => review.push(format!("Location eligibility not stated — confirm the employer can hire in {}", p.home_state)),
    }

    // Expiry
    if is_expired(d.valid_through.as_deref(), today) {
        review.push(format!("Posting may be expired (valid through {})", d.valid_through.as_deref().unwrap_or("")));
    }

    // Employment type
    if !d.employment_type.is_empty() && !p.employment_types.is_empty() {
        let wanted: Vec<String> = p.employment_types.iter().map(|t| t.to_lowercase()).collect();
        let any = d
            .employment_type
            .split(',')
            .map(|t| t.trim().to_lowercase())
            .any(|t| wanted.contains(&t));
        if !any {
            review.push(format!("Employment type ({}) is not in your preferences", d.employment_type));
        }
    }

    // User-defined exclusions
    let hay = format!("{}\n{}", d.title, d.description).to_lowercase();
    for kw in p.excluded_keywords.iter().map(|k| k.trim()).filter(|k| !k.is_empty()) {
        if hay.contains(&kw.to_lowercase()) {
            excluded.push(format!("Contains excluded keyword “{kw}”"));
        }
    }

    Evaluation { salary_flag: salary_flag.into(), review_flags: review, excluded_reasons: excluded }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> JobDraft {
        JobDraft {
            title: "Data Analyst".into(),
            employer: "Fabrikam".into(),
            remote_type: "remote".into(),
            wisconsin_eligibility: "eligible".into(),
            salary_min: Some(60000.0),
            salary_max: Some(70000.0),
            salary_period: Some("year".into()),
            employment_type: "Full-time".into(),
            ..Default::default()
        }
    }
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()
    }

    #[test]
    fn clean_match_has_no_flags() {
        let e = evaluate(&draft(), &Preferences::default(), today());
        assert_eq!(e.salary_flag, "meets");
        assert!(e.excluded_reasons.is_empty(), "{:?}", e.excluded_reasons);
        assert!(e.review_flags.is_empty(), "{:?}", e.review_flags);
    }

    #[test]
    fn missing_salary_is_flagged_not_assumed() {
        let mut d = draft();
        d.salary_min = None;
        d.salary_max = None;
        let e = evaluate(&d, &Preferences::default(), today());
        assert_eq!(e.salary_flag, "missing");
        assert!(e.excluded_reasons.is_empty());
        assert!(e.review_flags.iter().any(|f| f.contains("Salary not listed")));
    }

    #[test]
    fn hourly_is_annualised() {
        let mut d = draft();
        d.salary_min = Some(22.0);
        d.salary_max = Some(23.0); // 23 * 2080 = 47,840 < 50,000
        d.salary_period = Some("hour".into());
        let e = evaluate(&d, &Preferences::default(), today());
        assert_eq!(e.salary_flag, "below");
        assert!(e.excluded_reasons[0].contains("$47,840"));

        d.salary_min = Some(24.0); // 49,920
        d.salary_max = Some(30.0); // 62,400
        assert_eq!(evaluate(&d, &Preferences::default(), today()).salary_flag, "partial");
        d.salary_min = Some(25.0); // 52,000
        assert_eq!(evaluate(&d, &Preferences::default(), today()).salary_flag, "meets");
    }

    #[test]
    fn mandatory_violations_exclude() {
        let mut d = draft();
        d.remote_type = "hybrid".into();
        d.wisconsin_eligibility = "excluded".into();
        let e = evaluate(&d, &Preferences::default(), today());
        assert_eq!(e.excluded_reasons.len(), 2);
    }

    #[test]
    fn unknowns_need_review() {
        let mut d = draft();
        d.remote_type = "unknown".into();
        d.wisconsin_eligibility = "unknown".into();
        d.valid_through = Some("2026-10-01".into());
        d.employment_type = "Internship".into();
        let e = evaluate(&d, &Preferences::default(), today());
        assert!(e.excluded_reasons.is_empty());
        assert_eq!(e.review_flags.len(), 4, "{:?}", e.review_flags);
    }

    #[test]
    fn excluded_keywords_and_money_format() {
        let mut p = Preferences::default();
        p.excluded_keywords = vec!["security clearance".into()];
        let mut d = draft();
        d.description = "Active Security Clearance required".into();
        assert_eq!(evaluate(&d, &p, today()).excluded_reasons.len(), 1);
        assert_eq!(money(1234567.4), "$1,234,567");
        assert_eq!(money(950.0), "$950");
    }
}
