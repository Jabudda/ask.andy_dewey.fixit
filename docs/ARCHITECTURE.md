# Project Next Chapter — Architecture & Delivery Plan

## 1. Guiding principles

| Principle | How it is enforced |
|---|---|
| **Truthful** | The app never fabricates skills, metrics or answers. Every auto-detected value (salary, remote status, Wisconsin eligibility, title guesses) carries a *detection note* explaining where it came from, and the import screen is always an editable draft that the user saves. Skills are only "verified" when the user adds them to the profile. |
| **Human in control** | Status rules live in the Rust backend, not the UI: a job cannot become `Approved` unless it was `Awaiting Approval`, and cannot become `Submitted` without a user confirmation or a reference number. |
| **Local-first & private** | All data is stored in one SQLite file in `~/Library/Application Support/com.projectnextchapter.app/`. Nothing is sent anywhere except a URL the user explicitly asks the app to fetch. `.gitignore` blocks databases, documents, exports and `.env` files. |
| **Explainable** | Eligibility checks produce plain-English reasons (`excluded_reasons`, `review_flags`). Phase 2 scoring stores a per-criterion breakdown next to the score. |
| **Reliable** | SQLite WAL + transactions, versioned migrations (`PRAGMA user_version`), an app log table, a task queue table for resumable work (Phase 2+), online backup/restore via the SQLite backup API. |

## 2. System architecture

```
┌──────────────────────────── macOS app bundle (Tauri v2) ───────────────────────────┐
│                                                                                     │
│  React + TypeScript UI (WebView)            Rust core (src-tauri)                  │
│  ─────────────────────────────              ─────────────────────────────────       │
│  Dashboard · Opportunities · Job detail     commands.rs   ← typed IPC surface       │
│  Import · Career profile · Preferences      db.rs         ← SQLite repository       │
│  Data & logs                                parse.rs      ← JSON-LD + text parsing  │
│          │  invoke() (IPC, no network)      eligibility.rs← mandatory-rule checks   │
│          └────────────────────────────────▶ documents.rs  ← resume text extraction  │
│                                             export.rs     ← CSV/XLSX, backup        │
│                                             (P2) connectors/, scoring.rs, scheduler │
│                                             (P3) docgen/ (DOCX + PDF)               │
│                                             (P4) approval.rs, browser_assist.rs     │
│                                                    │                                 │
│                    SQLite (WAL) ◀──────────────────┤                                 │
│                    documents/ folder ◀─────────────┤                                 │
│                    macOS Keychain (P2: API keys) ◀─┘                                 │
└─────────────────────────────────────────────────────────────────────────────────────┘
          │ HTTPS (rustls) only when the user fetches a URL / enables a connector
          ▼
   Employer career pages (schema.org JobPosting), documented job APIs (P2),
   opt-in AI provider (P2/P3: Anthropic API or local Ollama), opt-in email (P4)
```

### Why these choices
* **Tauri v2** gives a ~10 MB native Apple Silicon bundle, a Rust backend with direct SQLite access, and a strict capability-based permission model (the UI can only open dialogs and URLs; it cannot touch the filesystem directly).
* **Rust owns all business rules** so tests cover the rules that matter (eligibility, duplicate detection, status guards) without a browser.
* **rusqlite (bundled SQLite)** avoids depending on the system SQLite version.
* **schema.org `JobPosting` JSON-LD** is the standard, publicly published structured data that employers and ATS platforms (Greenhouse, Lever, Workday, iCIMS, SmartRecruiters…) embed for search engines. Reading it from a page the user chose is a permitted, low-impact method — no login, no scraping of protected content, no CAPTCHA bypass.

## 3. Repository structure

```
project-next-chapter/
├── docs/ARCHITECTURE.md          ← this file
├── README.md                     ← install, build, test, backup, troubleshooting
├── sample-data/                  ← clearly labelled SAMPLE postings + sample resume
├── src/                          ← React UI
│   ├── App.tsx  main.tsx  styles.css
│   ├── lib/   api.ts (typed IPC) · types.ts · format.ts (+ format.test.ts)
│   ├── components/  JobForm.tsx · Charts.tsx · ui.tsx
│   └── pages/  Dashboard · Opportunities · JobDetail · ImportJob · Profile · Preferences · DataLogs
└── src-tauri/                    ← Rust core
    ├── Cargo.toml  tauri.conf.json  capabilities/default.json  icons/
    ├── migrations/001_init.sql
    └── src/ main.rs · lib.rs · commands.rs · db.rs · models.rs · error.rs
             parse.rs · eligibility.rs · documents.rs · export.rs
```

Planned additions: `src-tauri/src/connectors/{mod,greenhouse,lever,ashby,usajobs,remotive}.rs` (P2), `scoring.rs`, `scheduler.rs`, `secrets.rs` (Keychain, P2), `ai/{mod,anthropic,ollama}.rs` (P2/P3), `docgen/{docx,pdf}.rs` (P3), `approval.rs`, `browser_assist.rs`, `email.rs` (P4).

## 4. Database design (SQLite)

All tables are created in `migrations/001_init.sql`; later phases add migrations `002_…`, never edit old ones.

| Table | Purpose | Key columns |
|---|---|---|
| `settings` | Key/value app settings | `milestone_date` (default 2027-01-31), `preferences` (JSON) |
| `profile` | Single-row applicant identity | name, contact, location, headline, summary |
| `experience` | **Verified** employment history | employer, title, dates, `achievements` (one per line) |
| `skills` | **Verified** skills (user-confirmed) | name (unique, case-insensitive), category, evidence |
| `certifications` | Certifications | name, issuer, earned, expires, credential id |
| `accomplishments` | Accomplishment examples | title, description, evidence |
| `documents` | Uploaded + generated files | kind, file name, stored path, sha256, extracted text, `job_id`, `version` |
| `jobs` | Opportunity **and** its application | see below |
| `status_history` | Every status change (audit trail, analytics) | job_id, from, to, note, changed_at |
| `contacts` | Recruiter / hiring contacts | job_id, name, role, email, phone |
| `interviews` | Interview schedule & outcomes | job_id, scheduled_at, kind, outcome |
| `reminders` | Follow-up reminders | job_id, due_date, note, done |
| `app_log` | Searchable application log | ts, level, source, message |
| `task_queue` | Resumable background work (P2+) | kind, payload, state, attempts, last_error |
| `search_runs` | Scheduled-search reporting (P2) | connector, started/finished, found, new, error |

`jobs` columns: identity (`fingerprint` unique, `canonical_url`), provenance (`source`, `extraction_method`, `detection_notes`), posting data (title, employer, location, remote_type, wisconsin_eligibility, salary min/max/period, employment_type, posted_date, valid_through, apply_url, posting_url, description), evaluation (`salary_flag`, `review_flags`, `excluded_reasons`, `score`, `score_breakdown`, `assessment`), application (`status`, `notes`, `submitted_at`, `confirmation_ref`, `confirmation_method`), housekeeping (`is_sample`, `discovered_at`, `updated_at`).

Historical integrity: the milestone date is a single setting and is never written into job rows, so changing it does not alter history. Status analytics are computed from `status_history`, so "interviews reached" still counts after a job later moves to Rejected.

## 5. Status model

```
Discovered → Under Review → Documents Ready → Awaiting Approval ─┬→ Approved → In Progress → Submitted → Interviewing → Offer
                                                                  ├→ (Request changes) → Documents Ready
                                                                  ├→ (Save for later)  → Under Review
                                                                  └→ Rejected
Any status → Withdrawn / Closed / Rejected
```
Backend guards (enforced in `db::change_status`, covered by tests):
1. `Approved` only from `Awaiting Approval`.
2. `Submitted` only with explicit user confirmation or a confirmation/reference number; the method is recorded.

## 6. Phased implementation plan

| Phase | Scope | Exit criteria |
|---|---|---|
| **1 — Foundation** ✅ delivered | App shell, SQLite + migrations, career profile (resume upload with text extraction, experience, verified skills, certifications, accomplishments), preferences, job import (URL via JSON-LD, pasted text, manual), mandatory-rule checks (remote / Wisconsin / salary / expiry / excluded keywords), duplicate detection, tracking (statuses, history, notes, contacts, interviews, reminders, submission confirmation), dashboard + countdown, CSV/XLSX export, backup/restore, logs, sample data | All Rust + UI tests pass; app builds as a `.app` on Apple Silicon |
| **2 — Discovery & scoring** ✅ delivered (AI assessment and USAJOBS deferred) | Connectors for documented public APIs: Greenhouse Job Board API, Lever Postings API, Ashby public job board API, USAJOBS API (free key), Remotive API; employer watch-list; scheduler while app is open (+ optional LaunchAgent with explicit opt-in); `search_runs` reporting; fuzzy duplicate detection; configurable weighted 0–100 score with per-criterion breakdown; optional AI hiring-manager assessment (opt-in, cost disclosure, Keychain-stored key; local Ollama option) | Connector contract tests with recorded fixtures; scoring tests; failure reporting visible in UI |
| **3 — Documents** ✅ delivered | Tailored resume & cover letter from verified profile only; claim-check (every bullet must trace to a verified source); DOCX (docx-rs) + PDF generation; in-app editor; side-by-side diff vs master; version history in `documents` | Generated files open in Word/Preview; tests prove no unverified claims are inserted |
| **4 — Application assistance** ✅ delivered (core; browser form-filling and email monitoring still planned) | Awaiting Approval queue with job + fit + documents + proposed answers; Approve / Request changes / Save for later / Reject; open the official application page; optional permitted browser assistance that pauses on login/CAPTCHA/unknown questions; confirmation capture; follow-up reminders; opt-in Gmail/Outlook read-only monitoring | Nothing can submit without per-job approval (tested); Submitted requires observed or user-confirmed confirmation |
| **5 — Packaging & QA** | Signed/notarised DMG (requires the user's Apple Developer ID), updater, end-to-end tests, installation/backup/troubleshooting guides | Clean install on a fresh Mac |

## 7. Security & privacy

* No telemetry. Network access happens only on a user action: fetching a posting URL, running a discovery source, generating a document, or **Read website** for company research (`company.rs`: same-site pages only, robots.txt `User-agent: *` rules honoured, one request at a time, no cookies, nothing about the user in the request). All HTTP uses rustls TLS; only `http`/`https` URLs are accepted.
* The Tauri capability file grants the UI only `dialog` and `opener` (open URL in the default browser). File reads/writes happen in Rust on paths the user picked in a native dialog.
* API keys (Phase 2+) go into the macOS Keychain via the `keyring` crate — never into SQLite or the repo.
* CSV export neutralises spreadsheet formula injection (`=`, `+`, `-`, `@` prefixes).
* `.gitignore` excludes `*.sqlite`, `*.db`, `documents/`, `exports/`, `backups/`, `.env*`.
