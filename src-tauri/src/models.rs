use serde::{Deserialize, Serialize};

pub const STATUSES: [&str; 12] = [
    "Discovered",
    "Under Review",
    "Documents Ready",
    "Awaiting Approval",
    "Approved",
    "In Progress",
    "Submitted",
    "Interviewing",
    "Offer",
    "Rejected",
    "Withdrawn",
    "Closed",
];

/// An editable, not-yet-saved job posting produced by an importer or typed by the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct JobDraft {
    pub title: String,
    pub employer: String,
    pub location_text: String,
    /// remote | hybrid | onsite | unknown
    pub remote_type: String,
    /// eligible | excluded | unknown
    pub wisconsin_eligibility: String,
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    /// year | hour | day | week | month
    pub salary_period: Option<String>,
    pub employment_type: String,
    pub posted_date: Option<String>,
    pub valid_through: Option<String>,
    pub apply_url: Option<String>,
    pub posting_url: Option<String>,
    pub description: String,
    pub source: String,
    /// json-ld | heuristic | manual
    pub extraction_method: String,
    /// Where each auto-detected value came from, so the user can verify it.
    pub detection_notes: Vec<String>,
    /// The candidate already works for this employer (affects seniority scoring and the writing prompt).
    pub internal_candidate: bool,
    /// The candidate's own notes about this application (e.g. "lateral move"); sent to Claude, never cited as a fact.
    pub candidate_context: String,
}

impl Default for JobDraft {
    fn default() -> Self {
        JobDraft {
            title: String::new(),
            employer: String::new(),
            location_text: String::new(),
            remote_type: "unknown".into(),
            wisconsin_eligibility: "unknown".into(),
            salary_min: None,
            salary_max: None,
            salary_period: None,
            employment_type: String::new(),
            posted_date: None,
            valid_through: None,
            apply_url: None,
            posting_url: None,
            description: String::new(),
            source: "manual".into(),
            extraction_method: "manual".into(),
            detection_notes: vec![],
            internal_candidate: false,
            candidate_context: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: i64,
    #[serde(flatten)]
    pub draft: JobDraft,
    pub salary_flag: String,
    pub review_flags: Vec<String>,
    pub excluded_reasons: Vec<String>,
    pub expired: bool,
    pub status: String,
    pub score: Option<i64>,
    pub score_breakdown: Option<ScoreBreakdown>,
    pub assessment: Option<String>,
    pub notes: String,
    pub submitted_at: Option<String>,
    pub confirmation_ref: Option<String>,
    pub confirmation_method: Option<String>,
    pub is_sample: bool,
    pub discovered_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveJobResult {
    pub id: Option<i64>,
    pub duplicate_of: Option<Job>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusChange {
    pub job_id: i64,
    pub to_status: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub confirmation_ref: Option<String>,
    /// The user ticked "I confirm the employer received my application".
    #[serde(default)]
    pub user_confirmed_submission: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusEvent {
    pub id: i64,
    pub from_status: Option<String>,
    pub to_status: String,
    pub note: String,
    pub changed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Contact {
    pub id: Option<i64>,
    pub job_id: i64,
    pub name: String,
    pub role: String,
    pub email: String,
    pub phone: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Interview {
    pub id: Option<i64>,
    pub job_id: i64,
    pub scheduled_at: String,
    pub kind: String,
    pub notes: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Reminder {
    pub id: Option<i64>,
    pub job_id: i64,
    pub due_date: String,
    pub note: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDetail {
    pub job: Job,
    pub history: Vec<StatusEvent>,
    pub contacts: Vec<Contact>,
    pub interviews: Vec<Interview>,
    pub reminders: Vec<Reminder>,
    pub documents: Vec<DocumentRec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub home_city: String,
    pub home_state: String,
    pub remote_only: bool,
    pub min_annual_salary: f64,
    /// Used to convert hourly contract rates to an annual equivalent.
    pub hours_per_year: f64,
    pub employment_types: Vec<String>,
    pub target_titles: Vec<String>,
    pub preferred_fields: Vec<String>,
    pub keywords: Vec<String>,
    pub excluded_keywords: Vec<String>,
    pub follow_up_days: i64,
    /// Keyword searches sent to keyword-based boards (e.g. Remotive).
    pub search_queries: Vec<String>,
    /// Ignore discovered postings older than this.
    pub max_posting_age_days: i64,
    pub scoring_weights: ScoringWeights,
}

/// Relative importance of each compatibility criterion (any non-negative numbers; normalised).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ScoringWeights {
    pub title: f64,
    pub skills: f64,
    pub required: f64,
    pub preferred: f64,
    pub compensation: f64,
    pub remote: f64,
    pub seniority: f64,
    pub certifications: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        ScoringWeights { title: 15.0, skills: 25.0, required: 20.0, preferred: 5.0, compensation: 10.0, remote: 15.0, seniority: 5.0, certifications: 5.0 }
    }
}

impl Default for Preferences {
    fn default() -> Self {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        Preferences {
            home_city: "Milwaukee".into(),
            home_state: "Wisconsin".into(),
            remote_only: true,
            min_annual_salary: 50_000.0,
            hours_per_year: 2080.0,
            employment_types: v(&["Full-time", "Part-time", "Contract", "Temporary"]),
            target_titles: v(&[
                "Business Operations Analyst",
                "Operations Coordinator",
                "Data Analyst",
                "Reporting Analyst",
                "Project Coordinator",
                "IT Support Specialist",
                "Technical Support Specialist",
                "Smartsheet Administrator",
                "Process Improvement Analyst",
            ]),
            preferred_fields: v(&[
                "Business Operations",
                "Data Analysis",
                "Project Coordination",
                "IT Support",
                "Technical Support",
                "Smartsheet Administration",
                "Process Improvement",
            ]),
            keywords: v(&[
                "Smartsheet",
                "Excel",
                "Office Scripts",
                "SharePoint",
                "Microsoft Teams",
                "reporting",
                "data cleanup",
                "user acceptance testing",
                "documentation",
                "stakeholder",
                "crisis management",
                "troubleshooting",
            ]),
            excluded_keywords: vec![],
            follow_up_days: 14,
            search_queries: v(&[
                "smartsheet",
                "business operations",
                "operations coordinator",
                "project coordinator",
                "data analyst",
                "technical support",
                "process improvement",
            ]),
            max_posting_age_days: 30,
            scoring_weights: ScoringWeights::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileBasics {
    pub full_name: String,
    pub email: String,
    pub phone: String,
    pub location: String,
    pub linkedin_url: String,
    pub headline: String,
    pub summary: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Experience {
    pub id: Option<i64>,
    pub employer: String,
    pub title: String,
    pub location: String,
    pub start_date: String,
    pub end_date: String,
    pub is_current: bool,
    /// One verified achievement per line.
    pub achievements: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Skill {
    pub id: Option<i64>,
    pub name: String,
    pub category: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Certification {
    pub id: Option<i64>,
    pub name: String,
    pub issuer: String,
    pub date_earned: String,
    pub expires: String,
    pub credential_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Accomplishment {
    pub id: Option<i64>,
    pub title: String,
    pub description: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRec {
    pub id: i64,
    pub kind: String,
    pub file_name: String,
    pub stored_path: String,
    pub sha256: String,
    pub extracted_text: String,
    pub job_id: Option<i64>,
    pub version: i64,
    pub created_at: String,
    /// Generated documents only: structured content, claim-check findings, approval state.
    pub content_json: Option<serde_json::Value>,
    pub claim_report: Vec<crate::tailoring::Finding>,
    /// draft | approved | superseded | final
    pub status: String,
    pub approved_at: Option<String>,
    pub pdf_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Education {
    pub id: Option<i64>,
    pub institution: String,
    pub degree: String,
    pub field: String,
    pub minor: String,
    pub start_year: String,
    pub end_year: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct JobSource {
    pub id: Option<i64>,
    pub kind: String,
    pub identifier: String,
    pub label: String,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRun {
    pub id: i64,
    pub source_id: Option<i64>,
    pub connector: String,
    pub trigger: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub found: i64,
    pub new_jobs: i64,
    pub filtered: i64,
    pub excluded: i64,
    pub duplicates: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CriterionScore {
    pub key: String,
    pub label: String,
    pub weight: f64,
    /// 0.0 – 1.0
    pub score: f64,
    pub detail: String,
}

/// Stored as JSON in jobs.score_breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScoreBreakdown {
    pub total: i64,
    pub criteria: Vec<CriterionScore>,
    pub satisfied: Vec<String>,
    pub partial: Vec<String>,
    pub gaps: Vec<String>,
    /// Requirements the app could not judge from your profile — your call, not counted as met.
    pub unverified: Vec<String>,
    pub assumptions: Vec<String>,
    /// apply | consider | skip
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBundle {
    pub basics: ProfileBasics,
    pub experience: Vec<Experience>,
    pub education: Vec<Education>,
    pub skills: Vec<Skill>,
    pub certifications: Vec<Certification>,
    pub accomplishments: Vec<Accomplishment>,
    pub documents: Vec<DocumentRec>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentImportResult {
    pub document: DocumentRec,
    /// Skills found in the text that are not yet in the verified profile. Never added automatically.
    pub suggested_skills: Vec<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelCount {
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyActivity {
    pub week_start: String,
    pub discovered: i64,
    pub submitted: i64,
    pub interviews: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUp {
    pub job_id: i64,
    pub title: String,
    pub employer: String,
    pub reason: String,
    pub due: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub total_jobs: i64,
    pub strong_matches: i64,
    pub unscored: i64,
    pub awaiting_approval: i64,
    pub submitted: i64,
    pub interviews_scheduled: i64,
    pub offers: i64,
    pub rejections: i64,
    pub needs_follow_up: i64,
    pub excluded: i64,
    pub needs_review: i64,
    pub status_counts: Vec<LabelCount>,
    pub score_buckets: Vec<LabelCount>,
    pub activity: Vec<WeeklyActivity>,
    pub follow_ups: Vec<FollowUp>,
    /// Share of submitted applications that reached an interview. None until something is submitted.
    pub interview_rate: Option<f64>,
    pub offer_rate: Option<f64>,
    pub milestone_date: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: i64,
    pub ts: String,
    pub level: String,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub database_path: String,
    pub documents_dir: String,
}
