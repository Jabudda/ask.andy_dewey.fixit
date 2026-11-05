-- Company research: the employer's own language, read from its public website on request.
-- One row per employer (shared by its jobs); data_json holds pages read and the terms the user kept.

CREATE TABLE company_research (
    employer_key TEXT PRIMARY KEY,
    employer     TEXT NOT NULL,
    website      TEXT NOT NULL DEFAULT '',
    data_json    TEXT NOT NULL DEFAULT '{}',
    updated_at   TEXT NOT NULL
);
