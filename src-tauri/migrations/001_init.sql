-- Project Next Chapter — initial schema (Phase 1, with tables reserved for later phases).
-- Never edit a released migration; add 002_*.sql instead.

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO settings (key, value) VALUES ('milestone_date', '2027-01-31');

CREATE TABLE profile (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    full_name    TEXT NOT NULL DEFAULT '',
    email        TEXT NOT NULL DEFAULT '',
    phone        TEXT NOT NULL DEFAULT '',
    location     TEXT NOT NULL DEFAULT 'Milwaukee, Wisconsin',
    linkedin_url TEXT NOT NULL DEFAULT '',
    headline     TEXT NOT NULL DEFAULT '',
    summary      TEXT NOT NULL DEFAULT '',
    updated_at   TEXT
);
INSERT INTO profile (id) VALUES (1);

CREATE TABLE experience (
    id           INTEGER PRIMARY KEY,
    employer     TEXT NOT NULL,
    title        TEXT NOT NULL,
    location     TEXT NOT NULL DEFAULT '',
    start_date   TEXT NOT NULL DEFAULT '',
    end_date     TEXT NOT NULL DEFAULT '',
    is_current   INTEGER NOT NULL DEFAULT 0,
    achievements TEXT NOT NULL DEFAULT '',
    sort_order   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE skills (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL UNIQUE COLLATE NOCASE,
    category TEXT NOT NULL DEFAULT '',
    evidence TEXT NOT NULL DEFAULT ''
);

CREATE TABLE certifications (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL,
    issuer        TEXT NOT NULL DEFAULT '',
    date_earned   TEXT NOT NULL DEFAULT '',
    expires       TEXT NOT NULL DEFAULT '',
    credential_id TEXT NOT NULL DEFAULT ''
);

CREATE TABLE accomplishments (
    id          INTEGER PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    evidence    TEXT NOT NULL DEFAULT ''
);

CREATE TABLE jobs (
    id                    INTEGER PRIMARY KEY,
    fingerprint           TEXT NOT NULL,
    canonical_url         TEXT,
    source                TEXT NOT NULL,
    extraction_method     TEXT NOT NULL DEFAULT 'manual',
    detection_notes       TEXT NOT NULL DEFAULT '[]',
    title                 TEXT NOT NULL,
    employer              TEXT NOT NULL,
    location_text         TEXT NOT NULL DEFAULT '',
    remote_type           TEXT NOT NULL DEFAULT 'unknown',   -- remote | hybrid | onsite | unknown
    wisconsin_eligibility TEXT NOT NULL DEFAULT 'unknown',   -- eligible | excluded | unknown
    salary_min            REAL,
    salary_max            REAL,
    salary_period         TEXT,                              -- year | hour | day | week | month
    salary_flag           TEXT NOT NULL DEFAULT 'missing',   -- meets | partial | below | missing
    employment_type       TEXT NOT NULL DEFAULT '',
    posted_date           TEXT,
    valid_through         TEXT,
    apply_url             TEXT,
    posting_url           TEXT,
    description           TEXT NOT NULL DEFAULT '',
    review_flags          TEXT NOT NULL DEFAULT '[]',
    excluded_reasons      TEXT NOT NULL DEFAULT '[]',
    status                TEXT NOT NULL DEFAULT 'Discovered',
    score                 INTEGER,                           -- Phase 2
    score_breakdown       TEXT,                              -- Phase 2 (JSON)
    assessment            TEXT,                              -- Phase 2
    notes                 TEXT NOT NULL DEFAULT '',
    submitted_at          TEXT,
    confirmation_ref      TEXT,
    confirmation_method   TEXT,
    is_sample             INTEGER NOT NULL DEFAULT 0,
    discovered_at         TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_jobs_fingerprint ON jobs (fingerprint);
CREATE INDEX ix_jobs_canonical_url ON jobs (canonical_url);
CREATE INDEX ix_jobs_status ON jobs (status);

CREATE TABLE documents (
    id             INTEGER PRIMARY KEY,
    kind           TEXT NOT NULL,   -- master_resume | career_document | certification | accomplishment | generated_resume | generated_cover_letter
    file_name      TEXT NOT NULL,
    stored_path    TEXT NOT NULL,
    sha256         TEXT NOT NULL,
    extracted_text TEXT NOT NULL DEFAULT '',
    job_id         INTEGER REFERENCES jobs (id) ON DELETE SET NULL,
    version        INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL
);

CREATE TABLE status_history (
    id          INTEGER PRIMARY KEY,
    job_id      INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    from_status TEXT,
    to_status   TEXT NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    changed_at  TEXT NOT NULL
);
CREATE INDEX ix_history_job ON status_history (job_id);

CREATE TABLE contacts (
    id     INTEGER PRIMARY KEY,
    job_id INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    name   TEXT NOT NULL,
    role   TEXT NOT NULL DEFAULT '',
    email  TEXT NOT NULL DEFAULT '',
    phone  TEXT NOT NULL DEFAULT '',
    notes  TEXT NOT NULL DEFAULT ''
);

CREATE TABLE interviews (
    id           INTEGER PRIMARY KEY,
    job_id       INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    scheduled_at TEXT NOT NULL,
    kind         TEXT NOT NULL DEFAULT '',
    notes        TEXT NOT NULL DEFAULT '',
    outcome      TEXT NOT NULL DEFAULT ''
);

CREATE TABLE reminders (
    id       INTEGER PRIMARY KEY,
    job_id   INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    due_date TEXT NOT NULL,
    note     TEXT NOT NULL DEFAULT '',
    done     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE app_log (
    id      INTEGER PRIMARY KEY,
    ts      TEXT NOT NULL,
    level   TEXT NOT NULL,
    source  TEXT NOT NULL,
    message TEXT NOT NULL
);
CREATE INDEX ix_log_ts ON app_log (ts);

-- Reserved for Phase 2+: resumable background work and scheduled-search reporting.
CREATE TABLE task_queue (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL,
    payload    TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'pending',
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE search_runs (
    id          INTEGER PRIMARY KEY,
    connector   TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    found       INTEGER NOT NULL DEFAULT 0,
    new_jobs    INTEGER NOT NULL DEFAULT 0,
    error       TEXT
);
